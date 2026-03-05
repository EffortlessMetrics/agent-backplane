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
//! Deep tests for the emulation framework: strategies, engine, and capability integration.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition};
use abp_core::{Capability, CapabilityManifest, SupportLevel as CoreSupportLevel};
use abp_emulation::strategies::{
    StreamingEmulation, ThinkingEmulation, ToolUseEmulation, VisionEmulation,
};
use abp_emulation::{
    apply_emulation, can_emulate, compute_fidelity, default_strategy, EmulationConfig,
    EmulationEngine, EmulationEntry, EmulationReport, EmulationStrategy, FidelityLabel,
};

// ═══════════════════════════════════════════════════════════════════════════
// (a) Emulation strategy types — 10 tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn strategy_system_prompt_injection_constructible() {
    let s = EmulationStrategy::SystemPromptInjection {
        prompt: "test prompt".into(),
    };
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
}

#[test]
fn strategy_post_processing_constructible() {
    let s = EmulationStrategy::PostProcessing {
        detail: "validate json".into(),
    };
    assert!(matches!(s, EmulationStrategy::PostProcessing { .. }));
}

#[test]
fn strategy_disabled_constructible() {
    let s = EmulationStrategy::Disabled {
        reason: "unsafe".into(),
    };
    assert!(matches!(s, EmulationStrategy::Disabled { .. }));
}

#[test]
fn strategy_serde_roundtrip_all_variants() {
    let variants = vec![
        EmulationStrategy::SystemPromptInjection {
            prompt: "step by step".into(),
        },
        EmulationStrategy::PostProcessing {
            detail: "parse json".into(),
        },
        EmulationStrategy::Disabled {
            reason: "no sandbox".into(),
        },
    ];
    for original in &variants {
        let json = serde_json::to_string(original).unwrap();
        let decoded: EmulationStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(*original, decoded);
    }
}

#[test]
fn strategy_json_includes_type_tag() {
    let s = EmulationStrategy::SystemPromptInjection {
        prompt: "hi".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains(r#""type":"system_prompt_injection""#));

    let s2 = EmulationStrategy::PostProcessing { detail: "x".into() };
    let json2 = serde_json::to_string(&s2).unwrap();
    assert!(json2.contains(r#""type":"post_processing""#));

    let s3 = EmulationStrategy::Disabled { reason: "y".into() };
    let json3 = serde_json::to_string(&s3).unwrap();
    assert!(json3.contains(r#""type":"disabled""#));
}

#[test]
fn default_strategy_maps_known_capabilities() {
    // ExtendedThinking → SystemPromptInjection
    assert!(matches!(
        default_strategy(&Capability::ExtendedThinking),
        EmulationStrategy::SystemPromptInjection { .. }
    ));
    // StructuredOutputJsonSchema → PostProcessing
    assert!(matches!(
        default_strategy(&Capability::StructuredOutputJsonSchema),
        EmulationStrategy::PostProcessing { .. }
    ));
    // CodeExecution → Disabled
    assert!(matches!(
        default_strategy(&Capability::CodeExecution),
        EmulationStrategy::Disabled { .. }
    ));
}

#[test]
fn default_strategy_for_image_input_is_system_prompt() {
    let s = default_strategy(&Capability::ImageInput);
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
}

#[test]
fn default_strategy_for_stop_sequences_is_post_processing() {
    let s = default_strategy(&Capability::StopSequences);
    assert!(matches!(s, EmulationStrategy::PostProcessing { .. }));
}

#[test]
fn strategy_applicability_can_emulate_vs_disabled() {
    // Emulatable capabilities
    assert!(can_emulate(&Capability::ExtendedThinking));
    assert!(can_emulate(&Capability::StructuredOutputJsonSchema));
    assert!(can_emulate(&Capability::ImageInput));
    assert!(can_emulate(&Capability::StopSequences));

    // Non-emulatable capabilities
    assert!(!can_emulate(&Capability::CodeExecution));
    assert!(!can_emulate(&Capability::Streaming));
    assert!(!can_emulate(&Capability::ToolUse));
    assert!(!can_emulate(&Capability::ToolRead));
}

#[test]
fn named_strategy_constructors_return_expected_variants() {
    assert!(matches!(
        abp_emulation::emulate_structured_output(),
        EmulationStrategy::SystemPromptInjection { .. }
    ));
    assert!(matches!(
        abp_emulation::emulate_code_execution(),
        EmulationStrategy::SystemPromptInjection { .. }
    ));
    assert!(matches!(
        abp_emulation::emulate_extended_thinking(),
        EmulationStrategy::SystemPromptInjection { .. }
    ));
    assert!(matches!(
        abp_emulation::emulate_image_input(),
        EmulationStrategy::SystemPromptInjection { .. }
    ));
    assert!(matches!(
        abp_emulation::emulate_stop_sequences(),
        EmulationStrategy::PostProcessing { .. }
    ));
}

#[test]
fn strategy_equality_distinguishes_content() {
    let a = EmulationStrategy::SystemPromptInjection {
        prompt: "alpha".into(),
    };
    let b = EmulationStrategy::SystemPromptInjection {
        prompt: "beta".into(),
    };
    assert_ne!(a, b);

    let c = EmulationStrategy::SystemPromptInjection {
        prompt: "alpha".into(),
    };
    assert_eq!(a, c);
}

// ═══════════════════════════════════════════════════════════════════════════
// (b) Emulation engine behavior — 10 tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn engine_initialization_with_defaults() {
    let engine = EmulationEngine::with_defaults();
    // Should resolve something for any capability
    let s = engine.resolve_strategy(&Capability::ExtendedThinking);
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
}

#[test]
fn engine_register_emulation_via_config() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::CodeExecution,
        EmulationStrategy::SystemPromptInjection {
            prompt: "Simulate execution".into(),
        },
    );
    let engine = EmulationEngine::new(config);
    let s = engine.resolve_strategy(&Capability::CodeExecution);
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
}

#[test]
fn engine_check_missing_reports_emulatable_vs_disabled() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[
        Capability::ExtendedThinking,
        Capability::CodeExecution,
        Capability::StructuredOutputJsonSchema,
    ]);
    // ExtendedThinking + StructuredOutput are emulatable, CodeExecution is disabled
    assert_eq!(report.applied.len(), 2);
    assert_eq!(report.warnings.len(), 1);
    assert!(report.warnings[0].contains("CodeExecution"));
}

#[test]
fn engine_apply_injects_system_prompt_for_injection_strategy() {
    let mut conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "Base prompt"))
        .push(IrMessage::text(IrRole::User, "Hello"));

    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);

    assert_eq!(report.applied.len(), 1);
    let sys = conv.system_message().unwrap();
    assert!(sys.text_content().contains("Think step by step"));
}

#[test]
fn engine_emulation_produces_labeled_output_not_silent() {
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Do work"));

    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[
            Capability::ExtendedThinking,
            Capability::StructuredOutputJsonSchema,
        ],
        &mut conv,
    );

    // Every emulation is recorded — never silent
    assert_eq!(report.applied.len(), 2);
    for entry in &report.applied {
        assert!(!matches!(
            entry.strategy,
            EmulationStrategy::Disabled { .. }
        ));
    }
}

#[test]
fn engine_unsupported_emulation_returns_warning() {
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Execute code"));

    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::CodeExecution], &mut conv);

    assert!(report.applied.is_empty());
    assert!(!report.warnings.is_empty());
    assert!(report.warnings[0].contains("not emulated"));
}

#[test]
fn engine_multiple_simultaneous_emulations() {
    let mut conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "System"))
        .push(IrMessage::text(IrRole::User, "Request"));

    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[
            Capability::ExtendedThinking,
            Capability::StructuredOutputJsonSchema,
            Capability::ImageInput,
            Capability::StopSequences,
        ],
        &mut conv,
    );

    assert_eq!(report.applied.len(), 4);
    assert!(report.warnings.is_empty());
}

#[test]
fn engine_with_no_registered_emulations_uses_defaults() {
    let config = EmulationConfig::new();
    assert!(config.strategies.is_empty());

    let engine = EmulationEngine::new(config);
    // Still resolves to default strategy
    let s = engine.resolve_strategy(&Capability::ExtendedThinking);
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
}

#[test]
fn engine_config_override_takes_precedence_over_default() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::Disabled {
            reason: "user preference".into(),
        },
    );

    let engine = EmulationEngine::new(config);
    let s = engine.resolve_strategy(&Capability::ExtendedThinking);
    assert!(matches!(s, EmulationStrategy::Disabled { .. }));
}

#[test]
fn engine_empty_capability_list_produces_empty_report() {
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Hi"));
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[], &mut conv);
    assert!(report.is_empty());
    assert!(!report.has_unemulatable());
}

// ═══════════════════════════════════════════════════════════════════════════
// (c) Capability-emulation integration — 10 tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fidelity_native_capability_labeled_native() {
    let native_caps = vec![Capability::Streaming];
    let report = EmulationReport::default();
    let labels = compute_fidelity(&native_caps, &report);

    assert_eq!(
        labels.get(&Capability::Streaming),
        Some(&FidelityLabel::Native)
    );
}

#[test]
fn fidelity_emulated_capability_labeled_emulated() {
    let report = EmulationReport {
        applied: vec![EmulationEntry {
            capability: Capability::ExtendedThinking,
            strategy: EmulationStrategy::SystemPromptInjection {
                prompt: "Think".into(),
            },
        }],
        warnings: vec![],
    };
    let labels = compute_fidelity(&[], &report);

    match labels.get(&Capability::ExtendedThinking) {
        Some(FidelityLabel::Emulated { strategy }) => {
            assert!(matches!(
                strategy,
                EmulationStrategy::SystemPromptInjection { .. }
            ));
        }
        other => panic!("expected Emulated, got {other:?}"),
    }
}

#[test]
fn negotiation_result_includes_emulation_info() {
    use abp_capability::negotiate_capabilities;

    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::Streaming, CoreSupportLevel::Native);
    manifest.insert(Capability::CodeExecution, CoreSupportLevel::Emulated);

    let result = negotiate_capabilities(
        &[Capability::Streaming, Capability::CodeExecution],
        &manifest,
    );

    assert!(result.native.contains(&Capability::Streaming));
    // CodeExecution is Emulated in manifest → lands in emulated bucket
    assert!(!result.emulated.is_empty());
    assert_eq!(result.emulated[0].0, Capability::CodeExecution);
}

#[test]
fn compatibility_report_includes_emulation_status() {
    use abp_capability::{generate_report, negotiate_capabilities};

    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::Streaming, CoreSupportLevel::Native);
    manifest.insert(Capability::ToolUse, CoreSupportLevel::Emulated);

    let result = negotiate_capabilities(&[Capability::Streaming, Capability::ToolUse], &manifest);
    let report = generate_report(&result);

    assert!(report.compatible);
    assert_eq!(report.native_count, 1);
    assert_eq!(report.emulated_count, 1);
    assert!(report.summary.contains("emulated"));
}

#[test]
fn emulation_strategy_affects_quality_assessment_via_warnings() {
    use abp_capability::negotiate_capabilities;

    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::Vision, CoreSupportLevel::Emulated);

    let result = negotiate_capabilities(&[Capability::Vision], &manifest);

    // Vision emulation uses Approximate strategy → fidelity loss
    let warns = result.warnings();
    assert_eq!(warns.len(), 1);
    assert!(warns[0].1.has_fidelity_loss());
}

#[test]
fn missing_emulation_for_required_capability_makes_non_viable() {
    use abp_capability::negotiate_capabilities;

    let manifest = CapabilityManifest::new(); // empty manifest
    let result = negotiate_capabilities(&[Capability::Streaming], &manifest);

    assert!(!result.is_viable());
    assert!(!result.unsupported.is_empty());
}

#[test]
fn partial_emulation_coverage() {
    use abp_capability::negotiate_capabilities;

    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::Streaming, CoreSupportLevel::Native);
    // ToolUse not in manifest → unsupported

    let result = negotiate_capabilities(&[Capability::Streaming, Capability::ToolUse], &manifest);

    assert!(!result.is_viable()); // partial coverage fails viability
    assert_eq!(result.native.len(), 1);
    assert_eq!(result.unsupported.len(), 1);
}

#[test]
fn emulation_downgrade_from_native_to_emulated() {
    // If a manifest marks something as Emulated instead of Native,
    // negotiate should categorize it as emulated with a strategy.
    use abp_capability::negotiate_capabilities;

    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::ExtendedThinking, CoreSupportLevel::Emulated);

    let result = negotiate_capabilities(&[Capability::ExtendedThinking], &manifest);

    assert!(result.native.is_empty());
    assert_eq!(result.emulated.len(), 1);
    assert!(result.is_viable());
}

#[test]
fn cross_dialect_emulation_strategies_differ() {
    use abp_capability::default_emulation_strategy;

    // Different capabilities get different strategies
    let tool_strategy = default_emulation_strategy(&Capability::ToolUse);
    let vision_strategy = default_emulation_strategy(&Capability::Vision);
    let json_strategy = default_emulation_strategy(&Capability::StructuredOutputJsonSchema);

    assert_ne!(tool_strategy, vision_strategy);
    assert_ne!(tool_strategy, json_strategy);
}

#[test]
fn full_capability_negotiation_with_emulation_and_fidelity() {
    use abp_capability::negotiate_capabilities;

    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::Streaming, CoreSupportLevel::Native);
    manifest.insert(Capability::ExtendedThinking, CoreSupportLevel::Emulated);
    // CodeExecution absent → unsupported

    let neg_result = negotiate_capabilities(
        &[
            Capability::Streaming,
            Capability::ExtendedThinking,
            Capability::CodeExecution,
        ],
        &manifest,
    );

    assert!(!neg_result.is_viable()); // CodeExecution unsupported

    // Now run emulation engine on the missing capabilities from negotiation
    let engine = EmulationEngine::with_defaults();
    let missing: Vec<Capability> = neg_result.emulated.iter().map(|(c, _)| c.clone()).collect();
    let mut conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are helpful"))
        .push(IrMessage::text(IrRole::User, "Analyze this"));

    let emu_report = engine.apply(&missing, &mut conv);

    // ExtendedThinking should have been emulated
    assert_eq!(emu_report.applied.len(), 1);
    assert_eq!(
        emu_report.applied[0].capability,
        Capability::ExtendedThinking
    );

    // Compute fidelity: Streaming is native, ExtendedThinking is emulated
    let fidelity = compute_fidelity(&neg_result.native, &emu_report);
    assert_eq!(
        fidelity.get(&Capability::Streaming),
        Some(&FidelityLabel::Native)
    );
    assert!(matches!(
        fidelity.get(&Capability::ExtendedThinking),
        Some(FidelityLabel::Emulated { .. })
    ));
}

// ═══════════════════════════════════════════════════════════════════════════
// Bonus: strategies module deep tests (ThinkingEmulation, ToolUseEmulation,
// VisionEmulation, StreamingEmulation)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn thinking_emulation_brief_injects_concise_prompt() {
    let emu = ThinkingEmulation::brief();
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Question"));
    emu.inject(&mut conv);

    let sys = conv.system_message().unwrap();
    assert!(sys.text_content().contains("step by step"));
}

#[test]
fn thinking_emulation_standard_uses_xml_markers() {
    let emu = ThinkingEmulation::standard();
    let prompt = emu.prompt_text();
    assert!(prompt.contains("<thinking>"));
    assert!(prompt.contains("</thinking>"));
}

#[test]
fn thinking_emulation_detailed_includes_verification() {
    let emu = ThinkingEmulation::detailed();
    let prompt = emu.prompt_text();
    assert!(prompt.contains("Verify"));
}

#[test]
fn thinking_extract_separates_thinking_and_answer() {
    let text = "Some preamble <thinking>My reasoning here</thinking> Final answer.";
    let (thinking, answer) = ThinkingEmulation::extract_thinking(text);
    assert_eq!(thinking, "My reasoning here");
    assert!(answer.contains("Final answer."));
}

#[test]
fn thinking_extract_no_tags_returns_empty_thinking() {
    let text = "Just an answer with no thinking tags.";
    let (thinking, answer) = ThinkingEmulation::extract_thinking(text);
    assert!(thinking.is_empty());
    assert_eq!(answer, text);
}

#[test]
fn thinking_to_block_creates_ir_thinking_block() {
    let text = "<thinking>Step 1, Step 2</thinking>Answer";
    let block = ThinkingEmulation::to_thinking_block(text);
    assert!(block.is_some());
    match block.unwrap() {
        IrContentBlock::Thinking { text } => assert!(text.contains("Step 1")),
        _ => panic!("expected Thinking block"),
    }
}

#[test]
fn tool_use_emulation_generates_prompt_with_tool_info() {
    let tools = vec![IrToolDefinition {
        name: "read_file".into(),
        description: "Read a file".into(),
        parameters: serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}}}),
    }];
    let prompt = ToolUseEmulation::tools_to_prompt(&tools);
    assert!(prompt.contains("read_file"));
    assert!(prompt.contains("Read a file"));
    assert!(prompt.contains("<tool_call>"));
}

#[test]
fn tool_use_emulation_parses_tool_calls_from_text() {
    let text = r#"Here is the result:
<tool_call>
{"name": "read_file", "arguments": {"path": "/tmp/test.txt"}}
</tool_call>
Done."#;
    let calls = ToolUseEmulation::parse_tool_calls(text);
    assert_eq!(calls.len(), 1);
    let call = calls[0].as_ref().unwrap();
    assert_eq!(call.name, "read_file");
    assert_eq!(call.arguments["path"], "/tmp/test.txt");
}

#[test]
fn tool_use_emulation_extracts_text_outside_tool_calls() {
    let text = "Before <tool_call>{\"name\":\"x\",\"arguments\":{}}</tool_call> After";
    let outside = ToolUseEmulation::extract_text_outside_tool_calls(text);
    assert!(outside.contains("Before"));
    assert!(outside.contains("After"));
    assert!(!outside.contains("tool_call"));
}

#[test]
fn vision_emulation_replaces_images_with_placeholders() {
    let mut conv = IrConversation::new().push(IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text {
                text: "Look at this:".into(),
            },
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "base64data".into(),
            },
        ],
    ));

    let count = VisionEmulation::replace_images_with_placeholders(&mut conv);
    assert_eq!(count, 1);

    // Image should now be a text placeholder
    let msg = &conv.messages[0];
    assert_eq!(msg.content.len(), 2);
    let placeholder_text = match &msg.content[1] {
        IrContentBlock::Text { text } => text.clone(),
        _ => panic!("expected text placeholder"),
    };
    assert!(placeholder_text.contains("Image 1"));
    assert!(placeholder_text.contains("image/png"));
}

#[test]
fn streaming_emulation_splits_and_reassembles() {
    let emu = StreamingEmulation::new(10);
    let text = "Hello world, this is a test of streaming emulation.";
    let chunks = emu.split_into_chunks(text);

    assert!(chunks.len() > 1);
    assert!(chunks.last().unwrap().is_final);
    for (i, chunk) in chunks.iter().enumerate() {
        assert_eq!(chunk.index, i);
    }

    let reassembled = StreamingEmulation::reassemble(&chunks);
    assert_eq!(reassembled, text);
}

#[test]
fn streaming_emulation_fixed_split_preserves_content() {
    let emu = StreamingEmulation::new(5);
    let text = "abcdefghijklmnop";
    let chunks = emu.split_fixed(text);

    let reassembled = StreamingEmulation::reassemble(&chunks);
    assert_eq!(reassembled, text);
    assert_eq!(chunks.len(), 4); // 5+5+5+1
}

#[test]
fn streaming_emulation_empty_text_produces_single_final_chunk() {
    let emu = StreamingEmulation::default_chunk_size();
    let chunks = emu.split_into_chunks("");
    assert_eq!(chunks.len(), 1);
    assert!(chunks[0].is_final);
    assert!(chunks[0].content.is_empty());
}

#[test]
fn emulation_report_serde_roundtrip() {
    let report = EmulationReport {
        applied: vec![
            EmulationEntry {
                capability: Capability::ExtendedThinking,
                strategy: EmulationStrategy::SystemPromptInjection {
                    prompt: "Think".into(),
                },
            },
            EmulationEntry {
                capability: Capability::StructuredOutputJsonSchema,
                strategy: EmulationStrategy::PostProcessing {
                    detail: "Parse JSON".into(),
                },
            },
        ],
        warnings: vec!["CodeExecution disabled".into()],
    };

    let json = serde_json::to_string(&report).unwrap();
    let decoded: EmulationReport = serde_json::from_str(&json).unwrap();
    assert_eq!(report, decoded);
}

#[test]
fn emulation_config_serde_roundtrip() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::SystemPromptInjection {
            prompt: "reason".into(),
        },
    );
    config.set(
        Capability::CodeExecution,
        EmulationStrategy::Disabled {
            reason: "unsafe".into(),
        },
    );
    config.set(
        Capability::StructuredOutputJsonSchema,
        EmulationStrategy::PostProcessing {
            detail: "validate".into(),
        },
    );

    let json = serde_json::to_string(&config).unwrap();
    let decoded: EmulationConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(config, decoded);
}

#[test]
fn fidelity_label_serde_roundtrip() {
    let labels = vec![
        FidelityLabel::Native,
        FidelityLabel::Emulated {
            strategy: EmulationStrategy::SystemPromptInjection {
                prompt: "test".into(),
            },
        },
    ];
    for label in &labels {
        let json = serde_json::to_string(label).unwrap();
        let decoded: FidelityLabel = serde_json::from_str(&json).unwrap();
        assert_eq!(*label, decoded);
    }
}

#[test]
fn apply_emulation_free_function_matches_engine() {
    let config = EmulationConfig::new();
    let caps = vec![Capability::ExtendedThinking, Capability::CodeExecution];

    let mut conv1 = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "sys"))
        .push(IrMessage::text(IrRole::User, "hi"));
    let mut conv2 = conv1.clone();

    let report1 = apply_emulation(&config, &caps, &mut conv1);
    let report2 = EmulationEngine::new(config).apply(&caps, &mut conv2);

    assert_eq!(report1, report2);
    assert_eq!(conv1, conv2);
}
