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
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::useless_vec)]
//! Deep tests for the emulation engine covering strategy selection, pipeline,
//! label propagation, support-level semantics, feature detection, error handling,
//! silent degradation prevention, multi-emulation, metrics, configuration,
//! cross-dialect scenarios, and edge cases.

use std::collections::BTreeMap;

use abp_capability::{
    CapabilityRegistry, NegotiationResult, SupportLevel, check_capability, generate_report,
    negotiate, negotiate::NegotiationPolicy, negotiate::apply_policy, negotiate::pre_negotiate,
    negotiate_capabilities,
};
use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    SupportLevel as CoreSupportLevel,
    ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition},
};
use abp_emulation::{
    EmulationConfig, EmulationEngine, EmulationEntry, EmulationReport, EmulationStrategy,
    FidelityLabel, apply_emulation, can_emulate, compute_fidelity, default_strategy,
    emulate_code_execution, emulate_extended_thinking, emulate_image_input, emulate_stop_sequences,
    emulate_structured_output,
    strategies::{
        ParsedToolCall, StreamingEmulation, ThinkingEmulation, ToolUseEmulation, VisionEmulation,
    },
};

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn manifest(entries: &[(Capability, CoreSupportLevel)]) -> CapabilityManifest {
    entries.iter().cloned().collect()
}

fn require(caps: &[(Capability, MinSupport)]) -> CapabilityRequirements {
    CapabilityRequirements {
        required: caps
            .iter()
            .map(|(c, m)| CapabilityRequirement {
                capability: c.clone(),
                min_support: m.clone(),
            })
            .collect(),
    }
}

fn simple_conv() -> IrConversation {
    IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are helpful."))
        .push(IrMessage::text(IrRole::User, "Hello"))
}

fn user_only_conv() -> IrConversation {
    IrConversation::new().push(IrMessage::text(IrRole::User, "Hello"))
}

fn conv_with_image() -> IrConversation {
    let img_msg = IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text {
                text: "Describe this:".into(),
            },
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "aWdub3Jl".into(),
            },
        ],
    );
    IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You describe images."))
        .push(img_msg)
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Strategy Selection
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn strategy_native_passthrough_no_emulation_needed() {
    let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let result = negotiate_capabilities(&[Capability::Streaming], &m);
    assert_eq!(result.native.len(), 1);
    assert!(result.emulated.is_empty());
    assert!(result.unsupported.is_empty());
}

#[test]
fn strategy_emulated_selected_for_emulated_backend() {
    let m = manifest(&[(Capability::ToolUse, CoreSupportLevel::Emulated)]);
    let result = negotiate_capabilities(&[Capability::ToolUse], &m);
    assert!(result.native.is_empty());
    assert_eq!(result.emulated.len(), 1);
}

#[test]
fn strategy_unsupported_when_not_in_manifest() {
    let m = manifest(&[]);
    let result = negotiate_capabilities(&[Capability::Vision], &m);
    assert_eq!(result.unsupported.len(), 1);
}

#[test]
fn strategy_unsupported_when_explicitly_unsupported() {
    let m = manifest(&[(Capability::Audio, CoreSupportLevel::Unsupported)]);
    let result = negotiate_capabilities(&[Capability::Audio], &m);
    assert_eq!(result.unsupported.len(), 1);
}

#[test]
fn strategy_restricted_classified_as_emulated() {
    let m = manifest(&[(
        Capability::ToolBash,
        CoreSupportLevel::Restricted {
            reason: "sandboxed".into(),
        },
    )]);
    let result = negotiate_capabilities(&[Capability::ToolBash], &m);
    assert_eq!(result.emulated.len(), 1);
    assert!(result.native.is_empty());
}

#[test]
fn strategy_extended_thinking_default_is_system_prompt() {
    let s = default_strategy(&Capability::ExtendedThinking);
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
}

#[test]
fn strategy_structured_output_default_is_post_processing() {
    let s = default_strategy(&Capability::StructuredOutputJsonSchema);
    assert!(matches!(s, EmulationStrategy::PostProcessing { .. }));
}

#[test]
fn strategy_code_execution_default_is_disabled() {
    let s = default_strategy(&Capability::CodeExecution);
    assert!(matches!(s, EmulationStrategy::Disabled { .. }));
}

#[test]
fn strategy_image_input_default_is_system_prompt() {
    let s = default_strategy(&Capability::ImageInput);
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
}

#[test]
fn strategy_stop_sequences_default_is_post_processing() {
    let s = default_strategy(&Capability::StopSequences);
    assert!(matches!(s, EmulationStrategy::PostProcessing { .. }));
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Emulation Pipeline
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn pipeline_system_prompt_injection_appends_to_existing() {
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);

    let sys_text = conv.system_message().unwrap().text_content();
    assert!(sys_text.contains("Think step by step"));
    assert!(sys_text.contains("You are helpful."));
    assert_eq!(report.applied.len(), 1);
}

#[test]
fn pipeline_system_prompt_injection_creates_new_when_missing() {
    let mut conv = user_only_conv();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);

    assert_eq!(conv.messages[0].role, IrRole::System);
    assert!(
        conv.messages[0]
            .text_content()
            .contains("Think step by step")
    );
}

#[test]
fn pipeline_post_processing_does_not_mutate_conversation() {
    let original = simple_conv();
    let mut conv = original.clone();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::StructuredOutputJsonSchema], &mut conv);
    assert_eq!(conv, original);
}

#[test]
fn pipeline_post_processing_recorded_in_report() {
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::StructuredOutputJsonSchema], &mut conv);
    assert_eq!(report.applied.len(), 1);
    assert!(matches!(
        report.applied[0].strategy,
        EmulationStrategy::PostProcessing { .. }
    ));
}

#[test]
fn pipeline_disabled_generates_warning_not_applied() {
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::CodeExecution], &mut conv);
    assert!(report.applied.is_empty());
    assert_eq!(report.warnings.len(), 1);
    assert!(report.warnings[0].contains("not emulated"));
}

#[test]
fn pipeline_disabled_does_not_mutate_conversation() {
    let original = simple_conv();
    let mut conv = original.clone();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::CodeExecution], &mut conv);
    assert_eq!(conv, original);
}

#[test]
fn pipeline_thinking_emulation_inject_standard() {
    let mut conv = simple_conv();
    ThinkingEmulation::standard().inject(&mut conv);
    let sys = conv.system_message().unwrap().text_content();
    assert!(sys.contains("<thinking>"));
}

#[test]
fn pipeline_thinking_emulation_extract_tags() {
    let text = "Some preamble <thinking>step 1 then step 2</thinking> Final answer.";
    let (thinking, answer) = ThinkingEmulation::extract_thinking(text);
    assert_eq!(thinking, "step 1 then step 2");
    assert!(answer.contains("Final answer."));
}

#[test]
fn pipeline_thinking_to_block_returns_some() {
    let text = "<thinking>reasoning here</thinking>done";
    let block = ThinkingEmulation::to_thinking_block(text);
    assert!(block.is_some());
    if let Some(IrContentBlock::Thinking { text }) = block {
        assert_eq!(text, "reasoning here");
    }
}

#[test]
fn pipeline_thinking_to_block_returns_none_when_no_tags() {
    let block = ThinkingEmulation::to_thinking_block("no tags here");
    assert!(block.is_none());
}

#[test]
fn pipeline_tool_use_inject_tools_into_system() {
    let mut conv = simple_conv();
    let tools = vec![IrToolDefinition {
        name: "search".into(),
        description: "Searches the web".into(),
        parameters: serde_json::json!({"type": "object"}),
    }];
    ToolUseEmulation::inject_tools(&mut conv, &tools);
    let sys = conv.system_message().unwrap().text_content();
    assert!(sys.contains("search"));
    assert!(sys.contains("<tool_call>"));
}

#[test]
fn pipeline_tool_use_parse_valid_call() {
    let text = r#"Let me search. <tool_call>
{"name": "search", "arguments": {"query": "rust"}}
</tool_call>"#;
    let results = ToolUseEmulation::parse_tool_calls(text);
    assert_eq!(results.len(), 1);
    let call = results[0].as_ref().unwrap();
    assert_eq!(call.name, "search");
}

#[test]
fn pipeline_tool_use_parse_multiple_calls() {
    let text = r#"<tool_call>
{"name": "read", "arguments": {"path": "a.txt"}}
</tool_call> text <tool_call>
{"name": "write", "arguments": {"path": "b.txt"}}
</tool_call>"#;
    let results = ToolUseEmulation::parse_tool_calls(text);
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|r| r.is_ok()));
}

#[test]
fn pipeline_tool_use_to_tool_use_block() {
    let call = ParsedToolCall {
        name: "grep".into(),
        arguments: serde_json::json!({"pattern": "TODO"}),
    };
    let block = ToolUseEmulation::to_tool_use_block(&call, "call-1");
    assert!(
        matches!(block, IrContentBlock::ToolUse { id, name, .. } if id == "call-1" && name == "grep")
    );
}

#[test]
fn pipeline_vision_emulation_replaces_images() {
    let mut conv = conv_with_image();
    let count = VisionEmulation::apply(&mut conv);
    assert_eq!(count, 1);
    assert!(!VisionEmulation::has_images(&conv));
    let sys = conv.system_message().unwrap().text_content();
    assert!(sys.contains("does not support vision"));
}

#[test]
fn pipeline_streaming_split_and_reassemble() {
    let emu = StreamingEmulation::new(10);
    let text = "Hello world, this is a test of streaming emulation.";
    let chunks = emu.split_into_chunks(text);
    assert!(chunks.len() > 1);
    assert!(chunks.last().unwrap().is_final);
    let reassembled = StreamingEmulation::reassemble(&chunks);
    assert_eq!(reassembled, text);
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Label Propagation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn label_native_caps_get_native_fidelity() {
    let report = EmulationReport::default();
    let labels = compute_fidelity(&[Capability::Streaming], &report);
    assert_eq!(
        labels.get(&Capability::Streaming),
        Some(&FidelityLabel::Native)
    );
}

#[test]
fn label_emulated_caps_get_emulated_fidelity() {
    let report = EmulationReport {
        applied: vec![EmulationEntry {
            capability: Capability::ExtendedThinking,
            strategy: EmulationStrategy::SystemPromptInjection {
                prompt: "Think.".into(),
            },
        }],
        warnings: vec![],
    };
    let labels = compute_fidelity(&[], &report);
    let label = labels.get(&Capability::ExtendedThinking).unwrap();
    assert!(matches!(label, FidelityLabel::Emulated { .. }));
}

#[test]
fn label_mixed_native_and_emulated() {
    let report = EmulationReport {
        applied: vec![EmulationEntry {
            capability: Capability::ExtendedThinking,
            strategy: emulate_extended_thinking(),
        }],
        warnings: vec![],
    };
    let labels = compute_fidelity(&[Capability::Streaming], &report);
    assert_eq!(labels.len(), 2);
    assert_eq!(labels[&Capability::Streaming], FidelityLabel::Native);
    assert!(matches!(
        labels[&Capability::ExtendedThinking],
        FidelityLabel::Emulated { .. }
    ));
}

#[test]
fn label_emulated_strategy_preserved_in_fidelity() {
    let strategy = EmulationStrategy::PostProcessing {
        detail: "validate JSON".into(),
    };
    let report = EmulationReport {
        applied: vec![EmulationEntry {
            capability: Capability::StructuredOutputJsonSchema,
            strategy: strategy.clone(),
        }],
        warnings: vec![],
    };
    let labels = compute_fidelity(&[], &report);
    if let Some(FidelityLabel::Emulated {
        strategy: found_strat,
    }) = labels.get(&Capability::StructuredOutputJsonSchema)
    {
        assert_eq!(*found_strat, strategy);
    } else {
        panic!("expected Emulated fidelity");
    }
}

#[test]
fn label_warnings_not_included_in_fidelity() {
    let report = EmulationReport {
        applied: vec![],
        warnings: vec!["CodeExecution not emulated".into()],
    };
    let labels = compute_fidelity(&[], &report);
    assert!(labels.is_empty());
}

#[test]
fn label_report_entries_appear_in_report_applied() {
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[Capability::ExtendedThinking, Capability::ImageInput],
        &mut conv,
    );
    assert_eq!(report.applied.len(), 2);
    let caps: Vec<_> = report.applied.iter().map(|e| &e.capability).collect();
    assert!(caps.contains(&&Capability::ExtendedThinking));
    assert!(caps.contains(&&Capability::ImageInput));
}

#[test]
fn label_fidelity_serde_roundtrip() {
    let labels = BTreeMap::from([
        (Capability::Streaming, FidelityLabel::Native),
        (
            Capability::ExtendedThinking,
            FidelityLabel::Emulated {
                strategy: emulate_extended_thinking(),
            },
        ),
    ]);
    let json = serde_json::to_string(&labels).unwrap();
    let decoded: BTreeMap<Capability, FidelityLabel> = serde_json::from_str(&json).unwrap();
    assert_eq!(labels, decoded);
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Support Level Semantics
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn support_native_satisfies_native() {
    assert!(CoreSupportLevel::Native.satisfies(&MinSupport::Native));
}

#[test]
fn support_native_satisfies_emulated() {
    assert!(CoreSupportLevel::Native.satisfies(&MinSupport::Emulated));
}

#[test]
fn support_emulated_does_not_satisfy_native() {
    assert!(!CoreSupportLevel::Emulated.satisfies(&MinSupport::Native));
}

#[test]
fn support_emulated_satisfies_emulated() {
    assert!(CoreSupportLevel::Emulated.satisfies(&MinSupport::Emulated));
}

#[test]
fn support_unsupported_satisfies_nothing() {
    assert!(!CoreSupportLevel::Unsupported.satisfies(&MinSupport::Native));
    assert!(!CoreSupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}

#[test]
fn support_restricted_satisfies_emulated_not_native() {
    let restricted = CoreSupportLevel::Restricted {
        reason: "sandboxed".into(),
    };
    assert!(!restricted.satisfies(&MinSupport::Native));
    assert!(restricted.satisfies(&MinSupport::Emulated));
}

#[test]
fn support_negotiate_respects_min_support_native() {
    let m = manifest(&[(Capability::ToolUse, CoreSupportLevel::Emulated)]);
    let reqs = require(&[(Capability::ToolUse, MinSupport::Native)]);
    let result = negotiate(&m, &reqs);
    // Emulated does NOT satisfy Native min_support
    assert_eq!(result.unsupported.len(), 1);
    assert!(result.native.is_empty());
}

#[test]
fn support_negotiate_emulated_accepted_with_min_emulated() {
    let m = manifest(&[(Capability::ToolUse, CoreSupportLevel::Emulated)]);
    let reqs = require(&[(Capability::ToolUse, MinSupport::Emulated)]);
    let result = negotiate(&m, &reqs);
    assert!(result.unsupported.is_empty());
    assert_eq!(result.emulated.len(), 1);
}

#[test]
fn support_negotiate_native_accepted_with_min_emulated() {
    let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let reqs = require(&[(Capability::Streaming, MinSupport::Emulated)]);
    let result = negotiate(&m, &reqs);
    assert_eq!(result.native.len(), 1);
    assert!(result.unsupported.is_empty());
}

#[test]
fn support_negotiate_native_accepted_with_min_native() {
    let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let reqs = require(&[(Capability::Streaming, MinSupport::Native)]);
    let result = negotiate(&m, &reqs);
    assert_eq!(result.native.len(), 1);
}

#[test]
fn support_negotiate_unsupported_rejected_for_both_levels() {
    let m = manifest(&[(Capability::Audio, CoreSupportLevel::Unsupported)]);
    let reqs_native = require(&[(Capability::Audio, MinSupport::Native)]);
    let reqs_emulated = require(&[(Capability::Audio, MinSupport::Emulated)]);
    assert_eq!(negotiate(&m, &reqs_native).unsupported.len(), 1);
    assert_eq!(negotiate(&m, &reqs_emulated).unsupported.len(), 1);
}

#[test]
fn support_negotiate_missing_from_manifest_rejected() {
    let m: CapabilityManifest = BTreeMap::new();
    let reqs = require(&[(Capability::Vision, MinSupport::Emulated)]);
    let result = negotiate(&m, &reqs);
    assert_eq!(result.unsupported.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Feature Detection
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn detect_can_emulate_returns_true_for_emulatable() {
    assert!(can_emulate(&Capability::ExtendedThinking));
    assert!(can_emulate(&Capability::StructuredOutputJsonSchema));
    assert!(can_emulate(&Capability::ImageInput));
    assert!(can_emulate(&Capability::StopSequences));
}

#[test]
fn detect_can_emulate_returns_false_for_disabled() {
    assert!(!can_emulate(&Capability::CodeExecution));
    assert!(!can_emulate(&Capability::Streaming));
    assert!(!can_emulate(&Capability::ToolUse));
    assert!(!can_emulate(&Capability::ToolRead));
}

#[test]
fn detect_check_capability_native_in_manifest() {
    let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    assert_eq!(
        check_capability(&m, &Capability::Streaming),
        SupportLevel::Native
    );
}

#[test]
fn detect_check_capability_emulated_in_manifest() {
    let m = manifest(&[(Capability::CodeExecution, CoreSupportLevel::Emulated)]);
    let level = check_capability(&m, &Capability::CodeExecution);
    assert!(matches!(level, SupportLevel::Emulated { .. }));
}

#[test]
fn detect_check_capability_missing_from_manifest() {
    let m: CapabilityManifest = BTreeMap::new();
    let level = check_capability(&m, &Capability::Vision);
    assert!(matches!(level, SupportLevel::Unsupported { .. }));
}

#[test]
fn detect_registry_compare_finds_gaps() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg.compare("anthropic/claude-3.5-sonnet", "google/gemini-1.5-pro");
    assert!(result.is_some());
    let result = result.unwrap();
    // Claude has ExtendedThinking natively; Gemini doesn't
    assert!(
        result
            .unsupported_caps()
            .contains(&Capability::ExtendedThinking)
    );
}

#[test]
fn detect_registry_query_capability_across_backends() {
    let reg = CapabilityRegistry::with_defaults();
    let results = reg.query_capability(&Capability::ExtendedThinking);
    // At least one backend should have native, some unsupported
    let has_native = results
        .iter()
        .any(|(_, l)| matches!(l, SupportLevel::Native));
    let has_unsupported = results
        .iter()
        .any(|(_, l)| matches!(l, SupportLevel::Unsupported { .. }));
    assert!(has_native);
    assert!(has_unsupported);
}

#[test]
fn detect_engine_resolve_prefers_config_over_default() {
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

// ═══════════════════════════════════════════════════════════════════════════
// 6. Error Handling
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_strict_policy_rejects_unsupported() {
    let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let result = pre_negotiate(&[Capability::Streaming, Capability::Vision], &m);
    let err = apply_policy(&result, NegotiationPolicy::Strict).unwrap_err();
    assert_eq!(err.unsupported.len(), 1);
    assert!(err.to_string().contains("strict"));
}

#[test]
fn error_best_effort_rejects_unsupported() {
    let m = manifest(&[]);
    let result = pre_negotiate(&[Capability::Audio], &m);
    let err = apply_policy(&result, NegotiationPolicy::BestEffort).unwrap_err();
    assert_eq!(err.unsupported.len(), 1);
}

#[test]
fn error_permissive_passes_even_all_unsupported() {
    let m = manifest(&[]);
    let result = pre_negotiate(&[Capability::Audio, Capability::Vision], &m);
    assert!(apply_policy(&result, NegotiationPolicy::Permissive).is_ok());
}

#[test]
fn error_disabled_strategy_produces_warning_string() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[Capability::CodeExecution]);
    assert!(report.has_unemulatable());
    assert!(report.warnings[0].contains("Cannot safely emulate"));
}

#[test]
fn error_tool_call_parse_invalid_json() {
    let text = "<tool_call>\nnot json\n</tool_call>";
    let results = ToolUseEmulation::parse_tool_calls(text);
    assert_eq!(results.len(), 1);
    assert!(results[0].is_err());
}

#[test]
fn error_tool_call_parse_missing_name() {
    let text = r#"<tool_call>
{"arguments": {"x": 1}}
</tool_call>"#;
    let results = ToolUseEmulation::parse_tool_calls(text);
    assert_eq!(results.len(), 1);
    assert!(results[0].is_err());
    assert!(results[0].as_ref().unwrap_err().contains("missing 'name'"));
}

#[test]
fn error_tool_call_unclosed_tag() {
    let text = "<tool_call>\n{\"name\": \"x\", \"arguments\": {}}";
    let results = ToolUseEmulation::parse_tool_calls(text);
    assert_eq!(results.len(), 1);
    assert!(results[0].is_err());
    assert!(results[0].as_ref().unwrap_err().contains("unclosed"));
}

#[test]
fn error_negotiation_error_is_std_error() {
    use abp_capability::negotiate::NegotiationError;
    let err = NegotiationError {
        policy: NegotiationPolicy::Strict,
        unsupported: vec![(Capability::Streaming, "missing".into())],
        warnings: vec![],
    };
    let _: &dyn std::error::Error = &err;
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Silent Degradation Prevention
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn silent_every_emulation_recorded_in_report() {
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[Capability::ExtendedThinking, Capability::ImageInput],
        &mut conv,
    );
    // Each emulated capability is explicitly listed
    assert_eq!(report.applied.len(), 2);
    for entry in &report.applied {
        assert!(!matches!(
            entry.strategy,
            EmulationStrategy::Disabled { .. }
        ));
    }
}

#[test]
fn silent_disabled_never_silently_applied() {
    let mut conv = simple_conv();
    let original = conv.clone();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::CodeExecution], &mut conv);
    // Conversation unchanged, warning generated
    assert_eq!(conv, original);
    assert!(report.applied.is_empty());
    assert!(!report.warnings.is_empty());
}

#[test]
fn silent_report_empty_means_nothing_happened() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[]);
    assert!(report.is_empty());
    assert!(!report.has_unemulatable());
}

#[test]
fn silent_check_missing_mirrors_apply_classification() {
    let engine = EmulationEngine::with_defaults();
    let caps = [
        Capability::ExtendedThinking,
        Capability::CodeExecution,
        Capability::StructuredOutputJsonSchema,
    ];
    let check = engine.check_missing(&caps);
    let mut conv = simple_conv();
    let apply = engine.apply(&caps, &mut conv);
    assert_eq!(check.applied.len(), apply.applied.len());
    assert_eq!(check.warnings.len(), apply.warnings.len());
}

#[test]
fn silent_fidelity_labels_distinguish_native_from_emulated() {
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    let labels = compute_fidelity(&[Capability::Streaming], &report);
    assert_eq!(labels[&Capability::Streaming], FidelityLabel::Native);
    assert!(matches!(
        labels[&Capability::ExtendedThinking],
        FidelityLabel::Emulated { .. }
    ));
}

#[test]
fn silent_vision_emulation_injects_fallback_prompt() {
    let mut conv = conv_with_image();
    VisionEmulation::apply(&mut conv);
    let sys = conv.system_message().unwrap().text_content();
    assert!(sys.contains("does not support vision"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Multiple Emulations in a Single Request
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn multi_two_system_prompt_injections() {
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[Capability::ExtendedThinking, Capability::ImageInput],
        &mut conv,
    );
    assert_eq!(report.applied.len(), 2);
    let sys = conv.system_message().unwrap().text_content();
    assert!(sys.contains("Think step by step"));
    assert!(sys.contains("Image inputs"));
}

#[test]
fn multi_injection_plus_post_processing() {
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[
            Capability::ExtendedThinking,
            Capability::StructuredOutputJsonSchema,
        ],
        &mut conv,
    );
    assert_eq!(report.applied.len(), 2);
    let has_injection = report
        .applied
        .iter()
        .any(|e| matches!(e.strategy, EmulationStrategy::SystemPromptInjection { .. }));
    let has_post = report
        .applied
        .iter()
        .any(|e| matches!(e.strategy, EmulationStrategy::PostProcessing { .. }));
    assert!(has_injection);
    assert!(has_post);
}

#[test]
fn multi_mixed_applied_and_disabled() {
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[
            Capability::ExtendedThinking,
            Capability::CodeExecution,
            Capability::ImageInput,
            Capability::Streaming,
        ],
        &mut conv,
    );
    assert_eq!(report.applied.len(), 2); // thinking + image
    assert_eq!(report.warnings.len(), 2); // code exec + streaming
}

#[test]
fn multi_all_disabled_produces_only_warnings() {
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[
            Capability::CodeExecution,
            Capability::Streaming,
            Capability::ToolUse,
        ],
        &mut conv,
    );
    assert!(report.applied.is_empty());
    assert_eq!(report.warnings.len(), 3);
}

#[test]
fn multi_fidelity_labels_for_all_emulated() {
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[
            Capability::ExtendedThinking,
            Capability::ImageInput,
            Capability::StopSequences,
        ],
        &mut conv,
    );
    let labels = compute_fidelity(&[], &report);
    assert_eq!(labels.len(), 3);
    for (_, label) in &labels {
        assert!(matches!(label, FidelityLabel::Emulated { .. }));
    }
}

#[test]
fn multi_sequential_applies_are_cumulative() {
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    let r1 = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    let r2 = engine.apply(&[Capability::ImageInput], &mut conv);
    assert_eq!(r1.applied.len(), 1);
    assert_eq!(r2.applied.len(), 1);
    let sys = conv.system_message().unwrap().text_content();
    assert!(sys.contains("Think step by step"));
    assert!(sys.contains("Image inputs"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Emulation Metrics
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn metrics_report_counts_applied() {
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[Capability::ExtendedThinking, Capability::ImageInput],
        &mut conv,
    );
    assert_eq!(report.applied.len(), 2);
}

#[test]
fn metrics_report_counts_warnings() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[Capability::CodeExecution, Capability::Streaming]);
    assert_eq!(report.warnings.len(), 2);
}

#[test]
fn metrics_report_is_empty_for_no_input() {
    let report = EmulationReport::default();
    assert!(report.is_empty());
    assert!(!report.has_unemulatable());
}

#[test]
fn metrics_report_has_unemulatable_when_warnings_exist() {
    let report = EmulationReport {
        applied: vec![],
        warnings: vec!["something".into()],
    };
    assert!(report.has_unemulatable());
}

#[test]
fn metrics_negotiation_total_matches() {
    let m = manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Emulated),
    ]);
    let result = negotiate_capabilities(
        &[
            Capability::Streaming,
            Capability::ToolUse,
            Capability::Vision,
        ],
        &m,
    );
    assert_eq!(result.total(), 3);
    assert_eq!(
        result.native.len() + result.emulated.len() + result.unsupported.len(),
        3
    );
}

#[test]
fn metrics_compatibility_report_counts() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolUse],
        vec![Capability::Audio],
    );
    let report = generate_report(&result);
    assert_eq!(report.native_count, 1);
    assert_eq!(report.emulated_count, 1);
    assert_eq!(report.unsupported_count, 1);
    assert!(!report.compatible);
}

#[test]
fn metrics_vision_emulation_returns_image_count() {
    let mut conv = conv_with_image();
    let count = VisionEmulation::apply(&mut conv);
    assert_eq!(count, 1);
}

#[test]
fn metrics_streaming_chunk_count() {
    let emu = StreamingEmulation::new(5);
    let chunks = emu.split_into_chunks("Hello world!");
    assert!(chunks.len() >= 2);
    assert_eq!(chunks.last().unwrap().is_final, true);
    assert_eq!(chunks[0].index, 0);
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Configuration-Driven Emulation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn config_empty_uses_all_defaults() {
    let engine = EmulationEngine::with_defaults();
    let s = engine.resolve_strategy(&Capability::ExtendedThinking);
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
}

#[test]
fn config_override_disables_normally_emulatable() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::Disabled {
            reason: "user disabled".into(),
        },
    );
    let engine = EmulationEngine::new(config);
    let report = engine.check_missing(&[Capability::ExtendedThinking]);
    assert!(report.has_unemulatable());
    assert!(report.applied.is_empty());
}

#[test]
fn config_override_enables_normally_disabled() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::CodeExecution,
        EmulationStrategy::SystemPromptInjection {
            prompt: "Simulate code.".into(),
        },
    );
    let engine = EmulationEngine::new(config);
    let mut conv = simple_conv();
    let report = engine.apply(&[Capability::CodeExecution], &mut conv);
    assert_eq!(report.applied.len(), 1);
    assert!(report.warnings.is_empty());
}

#[test]
fn config_partial_override_leaves_others_default() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::PostProcessing {
            detail: "custom".into(),
        },
    );
    let engine = EmulationEngine::new(config);
    // ExtendedThinking overridden
    assert!(matches!(
        engine.resolve_strategy(&Capability::ExtendedThinking),
        EmulationStrategy::PostProcessing { .. }
    ));
    // ImageInput still uses default
    assert!(matches!(
        engine.resolve_strategy(&Capability::ImageInput),
        EmulationStrategy::SystemPromptInjection { .. }
    ));
}

#[test]
fn config_serde_roundtrip() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::SystemPromptInjection {
            prompt: "Think.".into(),
        },
    );
    config.set(
        Capability::CodeExecution,
        EmulationStrategy::Disabled {
            reason: "unsafe".into(),
        },
    );
    let json = serde_json::to_string(&config).unwrap();
    let decoded: EmulationConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(config, decoded);
}

#[test]
fn config_free_function_apply_emulation() {
    let config = EmulationConfig::new();
    let mut conv = simple_conv();
    let report = apply_emulation(&config, &[Capability::ExtendedThinking], &mut conv);
    assert_eq!(report.applied.len(), 1);
}

#[test]
fn config_set_overwrites_previous() {
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
    let engine = EmulationEngine::new(config);
    assert!(matches!(
        engine.resolve_strategy(&Capability::ExtendedThinking),
        EmulationStrategy::PostProcessing { .. }
    ));
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. Cross-Dialect Emulation Scenarios
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn cross_openai_tool_use_on_gemini_is_native() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg
        .negotiate_by_name("google/gemini-1.5-pro", &[Capability::ToolUse])
        .unwrap();
    assert_eq!(result.native, vec![Capability::ToolUse]);
}

#[test]
fn cross_claude_extended_thinking_unsupported_on_openai() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg
        .negotiate_by_name("openai/gpt-4o", &[Capability::ExtendedThinking])
        .unwrap();
    assert!(!result.unsupported.is_empty());
}

#[test]
fn cross_openai_audio_unsupported_on_claude() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg
        .negotiate_by_name("anthropic/claude-3.5-sonnet", &[Capability::Audio])
        .unwrap();
    assert_eq!(result.unsupported.len(), 1);
}

#[test]
fn cross_claude_pdf_unsupported_on_openai() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg
        .negotiate_by_name("openai/gpt-4o", &[Capability::PdfInput])
        .unwrap();
    assert_eq!(result.unsupported.len(), 1);
}

#[test]
fn cross_gemini_code_execution_emulated_on_openai() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg
        .negotiate_by_name("openai/gpt-4o", &[Capability::CodeExecution])
        .unwrap();
    assert_eq!(result.emulated.len(), 1);
}

#[test]
fn cross_openai_structured_output_emulated_on_claude() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg
        .negotiate_by_name(
            "anthropic/claude-3.5-sonnet",
            &[Capability::StructuredOutputJsonSchema],
        )
        .unwrap();
    // Claude has StructuredOutputJsonSchema as Emulated
    assert_eq!(result.emulated.len(), 1);
}

#[test]
fn cross_copilot_vision_is_emulated() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg
        .negotiate_by_name("github/copilot", &[Capability::Vision])
        .unwrap();
    assert_eq!(result.emulated.len(), 1);
}

#[test]
fn cross_kimi_extended_thinking_unsupported() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg
        .negotiate_by_name("moonshot/kimi", &[Capability::ExtendedThinking])
        .unwrap();
    assert_eq!(result.unsupported.len(), 1);
}

#[test]
fn cross_codex_has_tool_capabilities_native() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg
        .negotiate_by_name(
            "openai/codex",
            &[
                Capability::ToolRead,
                Capability::ToolWrite,
                Capability::ToolEdit,
                Capability::ToolBash,
            ],
        )
        .unwrap();
    assert_eq!(result.native.len(), 4);
}

#[test]
fn cross_compare_claude_to_openai_gaps() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg
        .compare("anthropic/claude-3.5-sonnet", "openai/gpt-4o")
        .unwrap();
    // Claude has ExtendedThinking native; OpenAI doesn't
    assert!(
        result
            .unsupported_caps()
            .contains(&Capability::ExtendedThinking)
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. Edge Cases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn edge_empty_capabilities_no_emulation() {
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[], &mut conv);
    assert!(report.is_empty());
}

#[test]
fn edge_empty_conversation() {
    let mut conv = IrConversation::new();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(report.applied.len(), 1);
    // System message created
    assert_eq!(conv.messages[0].role, IrRole::System);
}

#[test]
fn edge_all_native_no_emulation_needed() {
    let m = manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Native),
        (Capability::Vision, CoreSupportLevel::Native),
    ]);
    let result = negotiate_capabilities(
        &[
            Capability::Streaming,
            Capability::ToolUse,
            Capability::Vision,
        ],
        &m,
    );
    assert_eq!(result.native.len(), 3);
    assert!(result.emulated.is_empty());
    assert!(result.is_viable());
}

#[test]
fn edge_all_emulated() {
    let m = manifest(&[
        (Capability::ToolUse, CoreSupportLevel::Emulated),
        (Capability::Vision, CoreSupportLevel::Emulated),
        (Capability::CodeExecution, CoreSupportLevel::Emulated),
    ]);
    let result = negotiate_capabilities(
        &[
            Capability::ToolUse,
            Capability::Vision,
            Capability::CodeExecution,
        ],
        &m,
    );
    assert_eq!(result.emulated.len(), 3);
    assert!(result.native.is_empty());
    assert!(result.is_viable());
}

#[test]
fn edge_all_unsupported() {
    let m: CapabilityManifest = BTreeMap::new();
    let result = negotiate_capabilities(
        &[Capability::Audio, Capability::Vision, Capability::Streaming],
        &m,
    );
    assert_eq!(result.unsupported.len(), 3);
    assert!(!result.is_viable());
}

#[test]
fn edge_empty_manifest_empty_requirements_is_viable() {
    let m: CapabilityManifest = BTreeMap::new();
    let result = negotiate_capabilities(&[], &m);
    assert!(result.is_viable());
    assert_eq!(result.total(), 0);
}

#[test]
fn edge_streaming_split_empty_text() {
    let emu = StreamingEmulation::new(10);
    let chunks = emu.split_into_chunks("");
    assert_eq!(chunks.len(), 1);
    assert!(chunks[0].is_final);
    assert!(chunks[0].content.is_empty());
}

#[test]
fn edge_streaming_chunk_size_zero_becomes_one() {
    let emu = StreamingEmulation::new(0);
    assert_eq!(emu.chunk_size(), 1);
}

#[test]
fn edge_streaming_fixed_split_empty() {
    let emu = StreamingEmulation::new(5);
    let chunks = emu.split_fixed("");
    assert_eq!(chunks.len(), 1);
    assert!(chunks[0].is_final);
}

#[test]
fn edge_tool_call_no_calls_in_text() {
    let results = ToolUseEmulation::parse_tool_calls("No tool calls here.");
    assert!(results.is_empty());
}

#[test]
fn edge_tool_call_extract_text_outside() {
    let text = "Before <tool_call>\n{\"name\":\"x\",\"arguments\":{}}\n</tool_call> After";
    let outside = ToolUseEmulation::extract_text_outside_tool_calls(text);
    assert!(outside.contains("Before"));
    assert!(outside.contains("After"));
    assert!(!outside.contains("tool_call"));
}

#[test]
fn edge_vision_no_images_returns_zero() {
    let mut conv = simple_conv();
    let count = VisionEmulation::apply(&mut conv);
    assert_eq!(count, 0);
}

#[test]
fn edge_vision_has_images_true_when_present() {
    let conv = conv_with_image();
    assert!(VisionEmulation::has_images(&conv));
}

#[test]
fn edge_vision_has_images_false_when_absent() {
    let conv = simple_conv();
    assert!(!VisionEmulation::has_images(&conv));
}

#[test]
fn edge_thinking_extract_no_tags() {
    let (thinking, answer) = ThinkingEmulation::extract_thinking("No tags at all");
    assert!(thinking.is_empty());
    assert_eq!(answer, "No tags at all");
}

#[test]
fn edge_thinking_extract_empty_thinking() {
    let (thinking, answer) = ThinkingEmulation::extract_thinking("<thinking></thinking>Answer");
    assert!(thinking.is_empty());
    assert_eq!(answer, "Answer");
}

#[test]
fn edge_named_strategy_factories() {
    let s1 = emulate_structured_output();
    assert!(matches!(
        s1,
        EmulationStrategy::SystemPromptInjection { .. }
    ));
    let s2 = emulate_code_execution();
    assert!(matches!(
        s2,
        EmulationStrategy::SystemPromptInjection { .. }
    ));
    let s3 = emulate_extended_thinking();
    assert!(matches!(
        s3,
        EmulationStrategy::SystemPromptInjection { .. }
    ));
    let s4 = emulate_image_input();
    assert!(matches!(
        s4,
        EmulationStrategy::SystemPromptInjection { .. }
    ));
    let s5 = emulate_stop_sequences();
    assert!(matches!(s5, EmulationStrategy::PostProcessing { .. }));
}

#[test]
fn edge_serde_roundtrip_emulation_report() {
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
        warnings: vec!["code execution disabled".into()],
    };
    let json = serde_json::to_string(&report).unwrap();
    let decoded: EmulationReport = serde_json::from_str(&json).unwrap();
    assert_eq!(report, decoded);
}

#[test]
fn edge_serde_roundtrip_all_strategy_variants() {
    let strategies = vec![
        EmulationStrategy::SystemPromptInjection {
            prompt: "test".into(),
        },
        EmulationStrategy::PostProcessing {
            detail: "validate".into(),
        },
        EmulationStrategy::Disabled {
            reason: "nope".into(),
        },
    ];
    for s in &strategies {
        let json = serde_json::to_string(s).unwrap();
        let decoded: EmulationStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(*s, decoded);
    }
}

#[test]
fn edge_compatibility_report_fully_native() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming, Capability::ToolUse],
        vec![],
        vec![],
    );
    let report = generate_report(&result);
    assert!(report.compatible);
    assert_eq!(report.native_count, 2);
    assert_eq!(report.emulated_count, 0);
    assert_eq!(report.unsupported_count, 0);
    assert!(report.summary.contains("fully compatible"));
}

#[test]
fn edge_compatibility_report_all_unsupported() {
    let result =
        NegotiationResult::from_simple(vec![], vec![], vec![Capability::Audio, Capability::Vision]);
    let report = generate_report(&result);
    assert!(!report.compatible);
    assert_eq!(report.unsupported_count, 2);
    assert!(report.summary.contains("incompatible"));
}

#[test]
fn edge_negotiate_by_name_unknown_returns_none() {
    let reg = CapabilityRegistry::with_defaults();
    assert!(
        reg.negotiate_by_name("unknown/model", &[Capability::Streaming])
            .is_none()
    );
}

#[test]
fn edge_registry_six_defaults() {
    let reg = CapabilityRegistry::with_defaults();
    assert_eq!(reg.len(), 6);
    assert!(reg.contains("openai/gpt-4o"));
    assert!(reg.contains("anthropic/claude-3.5-sonnet"));
    assert!(reg.contains("google/gemini-1.5-pro"));
    assert!(reg.contains("moonshot/kimi"));
    assert!(reg.contains("openai/codex"));
    assert!(reg.contains("github/copilot"));
}

#[test]
fn edge_thinking_detail_all_variants() {
    let brief = ThinkingEmulation::brief();
    let standard = ThinkingEmulation::standard();
    let detailed = ThinkingEmulation::detailed();
    assert_ne!(brief.prompt_text(), standard.prompt_text());
    assert_ne!(standard.prompt_text(), detailed.prompt_text());
}

#[test]
fn edge_tool_format_result_success() {
    let result = ToolUseEmulation::format_tool_result("search", "found 3 items", false);
    assert!(result.contains("search"));
    assert!(result.contains("found 3 items"));
    assert!(!result.contains("error"));
}

#[test]
fn edge_tool_format_result_error() {
    let result = ToolUseEmulation::format_tool_result("search", "timeout", true);
    assert!(result.contains("error"));
    assert!(result.contains("timeout"));
}

#[test]
fn edge_tools_to_prompt_empty() {
    let prompt = ToolUseEmulation::tools_to_prompt(&[]);
    assert!(prompt.is_empty());
}

#[test]
fn edge_streaming_reassemble_empty() {
    let text = StreamingEmulation::reassemble(&[]);
    assert!(text.is_empty());
}

#[test]
fn edge_streaming_single_chunk() {
    let emu = StreamingEmulation::new(100);
    let chunks = emu.split_into_chunks("Short");
    assert_eq!(chunks.len(), 1);
    assert!(chunks[0].is_final);
    assert_eq!(chunks[0].content, "Short");
}
