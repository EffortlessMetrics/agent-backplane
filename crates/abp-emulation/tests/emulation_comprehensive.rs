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
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! Comprehensive tests for the ABP emulation framework.
//!
//! Covers: registration, execution, labeling, fidelity, failure,
//! chaining, metrics, configuration, native bypass, and edge cases.

use abp_core::Capability;
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition};
use abp_emulation::strategies::*;
use abp_emulation::*;

// ── Helpers ────────────────────────────────────────────────────────────

fn simple_conv() -> IrConversation {
    IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are helpful."))
        .push(IrMessage::text(IrRole::User, "Hello"))
}

fn bare_conv() -> IrConversation {
    IrConversation::new().push(IrMessage::text(IrRole::User, "Hello"))
}

fn multi_turn() -> IrConversation {
    IrConversation::new()
        .push(IrMessage::text(IrRole::System, "Base."))
        .push(IrMessage::text(IrRole::User, "Q1"))
        .push(IrMessage::text(IrRole::Assistant, "A1"))
        .push(IrMessage::text(IrRole::User, "Q2"))
}

fn image_conv() -> IrConversation {
    let img_msg = IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text {
                text: "Describe this image".into(),
            },
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "base64data".into(),
            },
        ],
    );
    IrConversation::new()
        .push(IrMessage::text(IrRole::System, "Helper."))
        .push(img_msg)
}

fn sample_tools() -> Vec<IrToolDefinition> {
    vec![
        IrToolDefinition {
            name: "search".into(),
            description: "Search the web".into(),
            parameters: serde_json::json!({"type": "object", "properties": {"q": {"type": "string"}}}),
        },
        IrToolDefinition {
            name: "calc".into(),
            description: "Calculate expression".into(),
            parameters: serde_json::json!({"type": "object", "properties": {"expr": {"type": "string"}}}),
        },
    ]
}

// ═══════════════════════════════════════════════════════════════════════
// 1. EMULATION REGISTRATION
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn reg_config_starts_empty() {
    let cfg = EmulationConfig::new();
    assert!(cfg.strategies.is_empty());
}

#[test]
fn reg_set_single_strategy() {
    let mut cfg = EmulationConfig::new();
    cfg.set(
        Capability::ExtendedThinking,
        EmulationStrategy::SystemPromptInjection {
            prompt: "custom".into(),
        },
    );
    assert_eq!(cfg.strategies.len(), 1);
    assert!(cfg.strategies.contains_key(&Capability::ExtendedThinking));
}

#[test]
fn reg_overwrite_existing_strategy() {
    let mut cfg = EmulationConfig::new();
    cfg.set(
        Capability::ExtendedThinking,
        EmulationStrategy::SystemPromptInjection {
            prompt: "v1".into(),
        },
    );
    cfg.set(
        Capability::ExtendedThinking,
        EmulationStrategy::Disabled {
            reason: "v2".into(),
        },
    );
    assert_eq!(cfg.strategies.len(), 1);
    assert!(matches!(
        cfg.strategies[&Capability::ExtendedThinking],
        EmulationStrategy::Disabled { .. }
    ));
}

#[test]
fn reg_multiple_capabilities() {
    let mut cfg = EmulationConfig::new();
    cfg.set(Capability::ExtendedThinking, emulate_extended_thinking());
    cfg.set(Capability::ImageInput, emulate_image_input());
    cfg.set(Capability::StopSequences, emulate_stop_sequences());
    assert_eq!(cfg.strategies.len(), 3);
}

#[test]
fn reg_engine_uses_registered_over_default() {
    let mut cfg = EmulationConfig::new();
    cfg.set(
        Capability::ExtendedThinking,
        EmulationStrategy::PostProcessing {
            detail: "custom post".into(),
        },
    );
    let engine = EmulationEngine::new(cfg);
    let s = engine.resolve_strategy(&Capability::ExtendedThinking);
    assert!(matches!(s, EmulationStrategy::PostProcessing { .. }));
}

// ═══════════════════════════════════════════════════════════════════════
// 2. EMULATION EXECUTION
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn exec_system_prompt_injection_appends_to_existing() {
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    let sys = conv.system_message().unwrap();
    let text = sys.text_content();
    assert!(text.contains("You are helpful."));
    assert!(text.contains("Think step by step"));
}

#[test]
fn exec_system_prompt_injection_creates_system_msg() {
    let mut conv = bare_conv();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(conv.messages[0].role, IrRole::System);
}

#[test]
fn exec_post_processing_does_not_mutate() {
    let original = bare_conv();
    let mut conv = original.clone();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::StructuredOutputJsonSchema], &mut conv);
    assert_eq!(conv, original);
}

#[test]
fn exec_disabled_does_not_mutate() {
    let original = bare_conv();
    let mut conv = original.clone();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::CodeExecution], &mut conv);
    assert_eq!(conv, original);
}

#[test]
fn exec_apply_emulation_free_fn() {
    let cfg = EmulationConfig::new();
    let mut conv = simple_conv();
    let report = apply_emulation(&cfg, &[Capability::ExtendedThinking], &mut conv);
    assert_eq!(report.applied.len(), 1);
}

#[test]
fn exec_thinking_brief_inject() {
    let mut conv = simple_conv();
    ThinkingEmulation::brief().inject(&mut conv);
    let text = conv.system_message().unwrap().text_content();
    assert!(text.contains("step by step"));
}

#[test]
fn exec_thinking_standard_inject() {
    let mut conv = simple_conv();
    ThinkingEmulation::standard().inject(&mut conv);
    let text = conv.system_message().unwrap().text_content();
    assert!(text.contains("<thinking>"));
}

#[test]
fn exec_thinking_detailed_inject() {
    let mut conv = simple_conv();
    ThinkingEmulation::detailed().inject(&mut conv);
    let text = conv.system_message().unwrap().text_content();
    assert!(text.contains("sub-problems"));
}

#[test]
fn exec_vision_replace_images() {
    let mut conv = image_conv();
    let count = VisionEmulation::replace_images_with_placeholders(&mut conv);
    assert_eq!(count, 1);
    let user_text = conv.messages[1].text_content();
    assert!(user_text.contains("[Image 1:"));
    assert!(user_text.contains("does not support vision"));
}

#[test]
fn exec_vision_full_apply() {
    let mut conv = image_conv();
    let count = VisionEmulation::apply(&mut conv);
    assert_eq!(count, 1);
    let sys_text = conv.system_message().unwrap().text_content();
    assert!(sys_text.contains("1 image(s)"));
}

// ═══════════════════════════════════════════════════════════════════════
// 3. EMULATION LABELING
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn label_applied_entries_record_capability() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = simple_conv();
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(report.applied[0].capability, Capability::ExtendedThinking);
}

#[test]
fn label_applied_entries_record_strategy() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = simple_conv();
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert!(matches!(
        report.applied[0].strategy,
        EmulationStrategy::SystemPromptInjection { .. }
    ));
}

#[test]
fn label_disabled_produces_warning_text() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = simple_conv();
    let report = engine.apply(&[Capability::CodeExecution], &mut conv);
    assert!(report.warnings[0].contains("CodeExecution"));
    assert!(report.warnings[0].contains("not emulated"));
}

#[test]
fn label_never_silent_degradation() {
    // Every emulation must appear in applied, every disabled in warnings.
    let engine = EmulationEngine::with_defaults();
    let mut conv = simple_conv();
    let caps = vec![
        Capability::ExtendedThinking,
        Capability::StructuredOutputJsonSchema,
        Capability::CodeExecution,
        Capability::Streaming,
    ];
    let report = engine.apply(&caps, &mut conv);
    assert_eq!(report.applied.len() + report.warnings.len(), caps.len());
}

#[test]
fn label_check_missing_matches_apply() {
    let engine = EmulationEngine::with_defaults();
    let caps = vec![Capability::ExtendedThinking, Capability::CodeExecution];
    let check = engine.check_missing(&caps);
    let mut conv = simple_conv();
    let apply = engine.apply(&caps, &mut conv);
    assert_eq!(check.applied.len(), apply.applied.len());
    assert_eq!(check.warnings.len(), apply.warnings.len());
}

// ═══════════════════════════════════════════════════════════════════════
// 4. EMULATION FIDELITY
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn fidelity_native_label() {
    let native = vec![Capability::Streaming];
    let report = EmulationReport::default();
    let labels = compute_fidelity(&native, &report);
    assert_eq!(labels[&Capability::Streaming], FidelityLabel::Native);
}

#[test]
fn fidelity_emulated_label() {
    let native = vec![];
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
fn fidelity_native_and_emulated_coexist() {
    let native = vec![Capability::Streaming, Capability::ToolUse];
    let report = EmulationReport {
        applied: vec![EmulationEntry {
            capability: Capability::ExtendedThinking,
            strategy: emulate_extended_thinking(),
        }],
        warnings: vec![],
    };
    let labels = compute_fidelity(&native, &report);
    assert_eq!(labels.len(), 3);
    assert_eq!(labels[&Capability::Streaming], FidelityLabel::Native);
    assert!(matches!(
        labels[&Capability::ExtendedThinking],
        FidelityLabel::Emulated { .. }
    ));
}

#[test]
fn fidelity_warnings_omitted_from_labels() {
    let native = vec![];
    let report = EmulationReport {
        applied: vec![],
        warnings: vec!["CodeExecution not emulated".into()],
    };
    let labels = compute_fidelity(&native, &report);
    assert!(labels.is_empty());
}

#[test]
fn fidelity_emulated_overrides_native_for_same_cap() {
    // If a cap appears in both native and emulated, emulated wins (last-writer).
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
fn fidelity_label_serde_roundtrip_native() {
    let label = FidelityLabel::Native;
    let json = serde_json::to_string(&label).unwrap();
    let decoded: FidelityLabel = serde_json::from_str(&json).unwrap();
    assert_eq!(label, decoded);
}

#[test]
fn fidelity_label_serde_roundtrip_emulated() {
    let label = FidelityLabel::Emulated {
        strategy: emulate_extended_thinking(),
    };
    let json = serde_json::to_string(&label).unwrap();
    let decoded: FidelityLabel = serde_json::from_str(&json).unwrap();
    assert_eq!(label, decoded);
}

// ═══════════════════════════════════════════════════════════════════════
// 5. EMULATION FAILURE
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn fail_disabled_cap_produces_warning() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[Capability::CodeExecution]);
    assert!(report.has_unemulatable());
}

#[test]
fn fail_disabled_cap_not_in_applied() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = simple_conv();
    let report = engine.apply(&[Capability::CodeExecution], &mut conv);
    assert!(report.applied.is_empty());
}

#[test]
fn fail_multiple_disabled_caps() {
    let engine = EmulationEngine::with_defaults();
    let caps = vec![
        Capability::CodeExecution,
        Capability::Streaming,
        Capability::ToolUse,
    ];
    let report = engine.check_missing(&caps);
    assert_eq!(report.warnings.len(), 3);
    assert!(report.applied.is_empty());
}

#[test]
fn fail_tool_read_disabled_by_default() {
    assert!(!can_emulate(&Capability::ToolRead));
}

#[test]
fn fail_graceful_with_empty_conversation() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = IrConversation::new();
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(report.applied.len(), 1);
    // System message was created even for empty conversation.
    assert_eq!(conv.messages.len(), 1);
    assert_eq!(conv.messages[0].role, IrRole::System);
}

#[test]
fn fail_explicit_disable_via_config() {
    let mut cfg = EmulationConfig::new();
    cfg.set(
        Capability::ExtendedThinking,
        EmulationStrategy::Disabled {
            reason: "user said no".into(),
        },
    );
    let engine = EmulationEngine::new(cfg);
    let mut conv = simple_conv();
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert!(report.applied.is_empty());
    assert_eq!(report.warnings.len(), 1);
    assert!(report.warnings[0].contains("user said no"));
}

// ═══════════════════════════════════════════════════════════════════════
// 6. MULTIPLE / CHAINED EMULATIONS
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn chain_two_system_prompt_injections() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = simple_conv();
    engine.apply(
        &[Capability::ExtendedThinking, Capability::ImageInput],
        &mut conv,
    );
    let text = conv.system_message().unwrap().text_content();
    assert!(text.contains("Think step by step"));
    assert!(text.contains("Image inputs"));
}

#[test]
fn chain_system_prompt_and_post_processing() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = simple_conv();
    let report = engine.apply(
        &[
            Capability::ExtendedThinking,
            Capability::StructuredOutputJsonSchema,
        ],
        &mut conv,
    );
    assert_eq!(report.applied.len(), 2);
    // Only system prompt injection modifies conversation.
    let text = conv.system_message().unwrap().text_content();
    assert!(text.contains("Think step by step"));
}

#[test]
fn chain_emulatable_and_disabled() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = simple_conv();
    let report = engine.apply(
        &[Capability::ExtendedThinking, Capability::CodeExecution],
        &mut conv,
    );
    assert_eq!(report.applied.len(), 1);
    assert_eq!(report.warnings.len(), 1);
}

#[test]
fn chain_all_emulatable_defaults() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = simple_conv();
    let caps = vec![
        Capability::ExtendedThinking,
        Capability::StructuredOutputJsonSchema,
        Capability::ImageInput,
        Capability::StopSequences,
    ];
    let report = engine.apply(&caps, &mut conv);
    assert_eq!(report.applied.len(), 4);
    assert!(report.warnings.is_empty());
}

#[test]
fn chain_preserves_ordering_in_report() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = simple_conv();
    let caps = vec![
        Capability::ImageInput,
        Capability::ExtendedThinking,
        Capability::StopSequences,
    ];
    let report = engine.apply(&caps, &mut conv);
    assert_eq!(report.applied[0].capability, Capability::ImageInput);
    assert_eq!(report.applied[1].capability, Capability::ExtendedThinking);
    assert_eq!(report.applied[2].capability, Capability::StopSequences);
}

#[test]
fn chain_sequential_applies_accumulate() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = simple_conv();
    let r1 = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    let r2 = engine.apply(&[Capability::ImageInput], &mut conv);
    assert_eq!(r1.applied.len(), 1);
    assert_eq!(r2.applied.len(), 1);
    let text = conv.system_message().unwrap().text_content();
    assert!(text.contains("Think step by step"));
    assert!(text.contains("Image inputs"));
}

// ═══════════════════════════════════════════════════════════════════════
// 7. EMULATION METRICS / REPORT INSPECTION
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn metrics_empty_report() {
    let report = EmulationReport::default();
    assert!(report.is_empty());
    assert!(!report.has_unemulatable());
}

#[test]
fn metrics_report_with_applied_not_empty() {
    let report = EmulationReport {
        applied: vec![EmulationEntry {
            capability: Capability::ExtendedThinking,
            strategy: emulate_extended_thinking(),
        }],
        warnings: vec![],
    };
    assert!(!report.is_empty());
    assert!(!report.has_unemulatable());
}

#[test]
fn metrics_report_with_warnings_not_empty() {
    let report = EmulationReport {
        applied: vec![],
        warnings: vec!["oops".into()],
    };
    assert!(!report.is_empty());
    assert!(report.has_unemulatable());
}

#[test]
fn metrics_count_applied_and_warnings() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = simple_conv();
    let caps = vec![
        Capability::ExtendedThinking,
        Capability::StructuredOutputJsonSchema,
        Capability::CodeExecution,
        Capability::Streaming,
        Capability::ImageInput,
    ];
    let report = engine.apply(&caps, &mut conv);
    // ExtendedThinking, StructuredOutput, ImageInput = 3 applied
    // CodeExecution, Streaming = 2 disabled
    assert_eq!(report.applied.len(), 3);
    assert_eq!(report.warnings.len(), 2);
}

#[test]
fn metrics_report_serde_roundtrip() {
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
        warnings: vec!["CodeExecution disabled".into()],
    };
    let json = serde_json::to_string(&report).unwrap();
    let decoded: EmulationReport = serde_json::from_str(&json).unwrap();
    assert_eq!(report, decoded);
}

// ═══════════════════════════════════════════════════════════════════════
// 8. CONFIGURATION
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn config_default_is_empty() {
    let cfg = EmulationConfig::default();
    assert!(cfg.strategies.is_empty());
}

#[test]
fn config_enable_normally_disabled_cap() {
    let mut cfg = EmulationConfig::new();
    cfg.set(
        Capability::CodeExecution,
        EmulationStrategy::SystemPromptInjection {
            prompt: "Simulate execution.".into(),
        },
    );
    let engine = EmulationEngine::new(cfg);
    let mut conv = bare_conv();
    let report = engine.apply(&[Capability::CodeExecution], &mut conv);
    assert_eq!(report.applied.len(), 1);
    assert!(report.warnings.is_empty());
    assert!(conv.system_message().is_some());
}

#[test]
fn config_disable_normally_enabled_cap() {
    let mut cfg = EmulationConfig::new();
    cfg.set(
        Capability::ExtendedThinking,
        EmulationStrategy::Disabled {
            reason: "not wanted".into(),
        },
    );
    let engine = EmulationEngine::new(cfg);
    let mut conv = simple_conv();
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert!(report.applied.is_empty());
    assert_eq!(report.warnings.len(), 1);
}

#[test]
fn config_change_strategy_type() {
    let mut cfg = EmulationConfig::new();
    // Change ExtendedThinking from SystemPromptInjection to PostProcessing.
    cfg.set(
        Capability::ExtendedThinking,
        EmulationStrategy::PostProcessing {
            detail: "Extract reasoning after response".into(),
        },
    );
    let engine = EmulationEngine::new(cfg);
    let original = simple_conv();
    let mut conv = original.clone();
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(report.applied.len(), 1);
    assert!(matches!(
        report.applied[0].strategy,
        EmulationStrategy::PostProcessing { .. }
    ));
    // PostProcessing should not mutate conversation.
    assert_eq!(conv, original);
}

#[test]
fn config_serde_roundtrip() {
    let mut cfg = EmulationConfig::new();
    cfg.set(Capability::ExtendedThinking, emulate_extended_thinking());
    cfg.set(
        Capability::CodeExecution,
        EmulationStrategy::Disabled {
            reason: "unsafe".into(),
        },
    );
    cfg.set(Capability::StopSequences, emulate_stop_sequences());
    let json = serde_json::to_string_pretty(&cfg).unwrap();
    let decoded: EmulationConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, decoded);
}

#[test]
fn config_deterministic_serialization() {
    let mut cfg = EmulationConfig::new();
    cfg.set(Capability::StopSequences, emulate_stop_sequences());
    cfg.set(Capability::ExtendedThinking, emulate_extended_thinking());
    cfg.set(Capability::ImageInput, emulate_image_input());
    let json1 = serde_json::to_string(&cfg).unwrap();
    let json2 = serde_json::to_string(&cfg).unwrap();
    // BTreeMap ensures deterministic key order.
    assert_eq!(json1, json2);
}

// ═══════════════════════════════════════════════════════════════════════
// 9. NATIVE BYPASS
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn bypass_native_cap_not_emulated() {
    // If a capability is natively supported, we should not emulate it.
    // The engine only processes capabilities it is asked to emulate.
    let native = vec![Capability::Streaming];
    let missing = vec![Capability::ExtendedThinking];
    let engine = EmulationEngine::with_defaults();
    let mut conv = simple_conv();
    let report = engine.apply(&missing, &mut conv);
    // Only the missing cap should appear in report.
    assert_eq!(report.applied.len(), 1);
    assert_eq!(report.applied[0].capability, Capability::ExtendedThinking);
    // Fidelity labels should show Streaming as native.
    let labels = compute_fidelity(&native, &report);
    assert_eq!(labels[&Capability::Streaming], FidelityLabel::Native);
    assert!(matches!(
        labels[&Capability::ExtendedThinking],
        FidelityLabel::Emulated { .. }
    ));
}

#[test]
fn bypass_empty_missing_list_no_emulation() {
    let engine = EmulationEngine::with_defaults();
    let original = simple_conv();
    let mut conv = original.clone();
    let report = engine.apply(&[], &mut conv);
    assert!(report.is_empty());
    assert_eq!(conv, original);
}

#[test]
fn bypass_all_native_nothing_emulated() {
    let native = vec![
        Capability::Streaming,
        Capability::ToolUse,
        Capability::ExtendedThinking,
    ];
    let report = EmulationReport::default();
    let labels = compute_fidelity(&native, &report);
    assert_eq!(labels.len(), 3);
    for label in labels.values() {
        assert_eq!(*label, FidelityLabel::Native);
    }
}

#[test]
fn bypass_check_missing_no_mutation() {
    let engine = EmulationEngine::with_defaults();
    // check_missing does not take a mutable conversation.
    let report = engine.check_missing(&[Capability::ExtendedThinking, Capability::CodeExecution]);
    assert_eq!(report.applied.len(), 1);
    assert_eq!(report.warnings.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════
// 10. EDGE CASES
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn edge_duplicate_capability_in_list() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = simple_conv();
    let report = engine.apply(
        &[Capability::ExtendedThinking, Capability::ExtendedThinking],
        &mut conv,
    );
    // Both are processed — dedup is caller's responsibility.
    assert_eq!(report.applied.len(), 2);
}

#[test]
fn edge_empty_conversation() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = IrConversation::new();
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(report.applied.len(), 1);
    assert_eq!(conv.messages.len(), 1);
}

#[test]
fn edge_multi_turn_system_prompt_not_duplicated() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = multi_turn();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    // Still only one system message, but with appended content.
    let sys_msgs: Vec<_> = conv
        .messages
        .iter()
        .filter(|m| m.role == IrRole::System)
        .collect();
    assert_eq!(sys_msgs.len(), 1);
}

#[test]
fn edge_strategy_variant_equality() {
    let a = EmulationStrategy::SystemPromptInjection {
        prompt: "test".into(),
    };
    let b = EmulationStrategy::SystemPromptInjection {
        prompt: "test".into(),
    };
    let c = EmulationStrategy::SystemPromptInjection {
        prompt: "other".into(),
    };
    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
fn edge_report_clone() {
    let report = EmulationReport {
        applied: vec![EmulationEntry {
            capability: Capability::ExtendedThinking,
            strategy: emulate_extended_thinking(),
        }],
        warnings: vec!["warn".into()],
    };
    let cloned = report.clone();
    assert_eq!(report, cloned);
}

#[test]
fn edge_engine_clone() {
    let mut cfg = EmulationConfig::new();
    cfg.set(Capability::ExtendedThinking, emulate_extended_thinking());
    let engine = EmulationEngine::new(cfg);
    let cloned = engine.clone();
    // Both should resolve the same strategy.
    assert_eq!(
        engine.resolve_strategy(&Capability::ExtendedThinking),
        cloned.resolve_strategy(&Capability::ExtendedThinking),
    );
}

#[test]
fn edge_config_debug_display() {
    let cfg = EmulationConfig::new();
    let debug = format!("{cfg:?}");
    assert!(debug.contains("EmulationConfig"));
}

#[test]
fn edge_vision_no_images_noop() {
    let mut conv = simple_conv();
    let count = VisionEmulation::apply(&mut conv);
    assert_eq!(count, 0);
    // System message should not mention images.
    let text = conv.system_message().unwrap().text_content();
    assert!(!text.contains("image(s)"));
}

#[test]
fn edge_vision_has_images_check() {
    let conv = image_conv();
    assert!(VisionEmulation::has_images(&conv));
    let conv2 = simple_conv();
    assert!(!VisionEmulation::has_images(&conv2));
}

#[test]
fn edge_vision_multiple_images() {
    let img1 = IrContentBlock::Image {
        media_type: "image/png".into(),
        data: "data1".into(),
    };
    let img2 = IrContentBlock::Image {
        media_type: "image/jpeg".into(),
        data: "data2".into(),
    };
    let msg = IrMessage::new(IrRole::User, vec![img1, img2]);
    let mut conv = IrConversation::new().push(msg);
    let count = VisionEmulation::replace_images_with_placeholders(&mut conv);
    assert_eq!(count, 2);
    let text = conv.messages[0].text_content();
    assert!(text.contains("[Image 1:"));
    assert!(text.contains("[Image 2:"));
}

#[test]
fn edge_streaming_empty_text() {
    let emu = StreamingEmulation::default_chunk_size();
    let chunks = emu.split_into_chunks("");
    assert_eq!(chunks.len(), 1);
    assert!(chunks[0].is_final);
    assert!(chunks[0].content.is_empty());
}

#[test]
fn edge_streaming_reassemble_matches_original() {
    let text = "Hello world, this is a test of streaming emulation.";
    let emu = StreamingEmulation::new(10);
    let chunks = emu.split_into_chunks(text);
    let reassembled = StreamingEmulation::reassemble(&chunks);
    assert_eq!(reassembled, text);
}

#[test]
fn edge_streaming_fixed_reassemble() {
    let text = "abcdefghij1234567890";
    let emu = StreamingEmulation::new(5);
    let chunks = emu.split_fixed(text);
    let reassembled = StreamingEmulation::reassemble(&chunks);
    assert_eq!(reassembled, text);
}

#[test]
fn edge_streaming_chunk_indices_sequential() {
    let emu = StreamingEmulation::new(3);
    let chunks = emu.split_fixed("abcdefghi");
    for (i, chunk) in chunks.iter().enumerate() {
        assert_eq!(chunk.index, i);
    }
    assert!(chunks.last().unwrap().is_final);
}

#[test]
fn edge_streaming_minimum_chunk_size() {
    let emu = StreamingEmulation::new(0);
    assert_eq!(emu.chunk_size(), 1);
}

#[test]
fn edge_tool_use_parse_valid() {
    let text = r#"Here: <tool_call>
{"name": "search", "arguments": {"q": "rust"}}
</tool_call>"#;
    let results = ToolUseEmulation::parse_tool_calls(text);
    assert_eq!(results.len(), 1);
    let call = results[0].as_ref().unwrap();
    assert_eq!(call.name, "search");
    assert_eq!(call.arguments["q"], "rust");
}

#[test]
fn edge_tool_use_parse_multiple() {
    let text = r#"<tool_call>
{"name": "search", "arguments": {"q": "a"}}
</tool_call>
<tool_call>
{"name": "calc", "arguments": {"expr": "1+1"}}
</tool_call>"#;
    let results = ToolUseEmulation::parse_tool_calls(text);
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].as_ref().unwrap().name, "search");
    assert_eq!(results[1].as_ref().unwrap().name, "calc");
}

#[test]
fn edge_tool_use_parse_invalid_json() {
    let text = "<tool_call>not json</tool_call>";
    let results = ToolUseEmulation::parse_tool_calls(text);
    assert_eq!(results.len(), 1);
    assert!(results[0].is_err());
}

#[test]
fn edge_tool_use_parse_missing_name() {
    let text = r#"<tool_call>{"arguments": {"x": 1}}</tool_call>"#;
    let results = ToolUseEmulation::parse_tool_calls(text);
    assert_eq!(results.len(), 1);
    assert!(results[0].is_err());
}

#[test]
fn edge_tool_use_unclosed_tag() {
    let text = "<tool_call>{\"name\": \"x\", \"arguments\": {}}";
    let results = ToolUseEmulation::parse_tool_calls(text);
    assert_eq!(results.len(), 1);
    assert!(results[0].is_err());
    assert!(results[0].as_ref().unwrap_err().contains("unclosed"));
}

#[test]
fn edge_tool_use_inject_tools_prompt() {
    let tools = sample_tools();
    let mut conv = simple_conv();
    ToolUseEmulation::inject_tools(&mut conv, &tools);
    let text = conv.system_message().unwrap().text_content();
    assert!(text.contains("search"));
    assert!(text.contains("calc"));
    assert!(text.contains("<tool_call>"));
}

#[test]
fn edge_tool_use_empty_tools_noop() {
    let original = simple_conv();
    let mut conv = original.clone();
    ToolUseEmulation::inject_tools(&mut conv, &[]);
    assert_eq!(conv, original);
}

#[test]
fn edge_tool_use_to_block() {
    let call = ParsedToolCall {
        name: "search".into(),
        arguments: serde_json::json!({"q": "test"}),
    };
    let block = ToolUseEmulation::to_tool_use_block(&call, "id-1");
    match block {
        IrContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "id-1");
            assert_eq!(name, "search");
            assert_eq!(input["q"], "test");
        }
        _ => panic!("Expected ToolUse block"),
    }
}

#[test]
fn edge_tool_use_format_result_success() {
    let result = ToolUseEmulation::format_tool_result("calc", "42", false);
    assert!(result.contains("calc"));
    assert!(result.contains("42"));
    assert!(!result.contains("error"));
}

#[test]
fn edge_tool_use_format_result_error() {
    let result = ToolUseEmulation::format_tool_result("calc", "division by zero", true);
    assert!(result.contains("error"));
    assert!(result.contains("division by zero"));
}

#[test]
fn edge_tool_use_extract_text_outside() {
    let text = "Before <tool_call>{\"name\":\"x\",\"arguments\":{}}</tool_call> After";
    let outside = ToolUseEmulation::extract_text_outside_tool_calls(text);
    assert!(outside.contains("Before"));
    assert!(outside.contains("After"));
    assert!(!outside.contains("tool_call"));
}

#[test]
fn edge_thinking_extract_with_tags() {
    let text = "<thinking>My reasoning here</thinking>The answer is 42.";
    let (thinking, answer) = ThinkingEmulation::extract_thinking(text);
    assert_eq!(thinking, "My reasoning here");
    assert_eq!(answer, "The answer is 42.");
}

#[test]
fn edge_thinking_extract_no_tags() {
    let text = "Just a plain answer.";
    let (thinking, answer) = ThinkingEmulation::extract_thinking(text);
    assert!(thinking.is_empty());
    assert_eq!(answer, "Just a plain answer.");
}

#[test]
fn edge_thinking_to_block_some() {
    let text = "<thinking>reason</thinking>answer";
    let block = ThinkingEmulation::to_thinking_block(text);
    assert!(block.is_some());
    match block.unwrap() {
        IrContentBlock::Thinking { text } => assert_eq!(text, "reason"),
        _ => panic!("Expected Thinking block"),
    }
}

#[test]
fn edge_thinking_to_block_none() {
    let block = ThinkingEmulation::to_thinking_block("no thinking tags");
    assert!(block.is_none());
}

#[test]
fn edge_can_emulate_image_input() {
    assert!(can_emulate(&Capability::ImageInput));
}

#[test]
fn edge_can_emulate_stop_sequences() {
    assert!(can_emulate(&Capability::StopSequences));
}

#[test]
fn edge_default_strategy_all_three_variants_covered() {
    // Verify the three strategy types are all reachable.
    let s1 = default_strategy(&Capability::ExtendedThinking);
    assert!(matches!(
        s1,
        EmulationStrategy::SystemPromptInjection { .. }
    ));
    let s2 = default_strategy(&Capability::StructuredOutputJsonSchema);
    assert!(matches!(s2, EmulationStrategy::PostProcessing { .. }));
    let s3 = default_strategy(&Capability::CodeExecution);
    assert!(matches!(s3, EmulationStrategy::Disabled { .. }));
}

#[test]
fn edge_strategy_serde_tagged_type_field() {
    let s = EmulationStrategy::SystemPromptInjection {
        prompt: "test".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    // Verify serde tag format uses "type" field.
    let val: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(val["type"], "system_prompt_injection");
}

#[test]
fn edge_fidelity_label_serde_tagged_fidelity_field() {
    let label = FidelityLabel::Native;
    let json = serde_json::to_string(&label).unwrap();
    let val: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(val["fidelity"], "native");
}

#[test]
fn edge_streaming_word_boundary_preference() {
    let text = "Hello world foo bar";
    let emu = StreamingEmulation::new(12);
    let chunks = emu.split_into_chunks(text);
    // Should prefer splitting at word boundaries.
    let reassembled = StreamingEmulation::reassemble(&chunks);
    assert_eq!(reassembled, text);
    // First chunk should end at a word boundary.
    assert!(chunks[0].content.ends_with(' ') || chunks[0].content == text);
}
