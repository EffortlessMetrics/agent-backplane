// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for the emulation engine.

use abp_core::Capability;
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition};
use abp_emulation::strategies::*;
use abp_emulation::*;

// ── Factory function output tests ──────────────────────────────────────

#[test]
fn emulate_structured_output_returns_system_prompt_injection() {
    let s = emulate_structured_output();
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
    if let EmulationStrategy::SystemPromptInjection { prompt } = &s {
        assert!(prompt.contains("JSON"));
    }
}

#[test]
fn emulate_code_execution_returns_system_prompt_injection() {
    let s = emulate_code_execution();
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
    if let EmulationStrategy::SystemPromptInjection { prompt } = &s {
        assert!(prompt.contains("execute code"));
    }
}

#[test]
fn emulate_extended_thinking_returns_system_prompt_injection() {
    let s = emulate_extended_thinking();
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
    if let EmulationStrategy::SystemPromptInjection { prompt } = &s {
        assert!(prompt.contains("step by step"));
    }
}

#[test]
fn emulate_image_input_returns_system_prompt_injection() {
    let s = emulate_image_input();
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
    if let EmulationStrategy::SystemPromptInjection { prompt } = &s {
        assert!(prompt.contains("Image"));
    }
}

#[test]
fn emulate_stop_sequences_returns_post_processing() {
    let s = emulate_stop_sequences();
    assert!(matches!(s, EmulationStrategy::PostProcessing { .. }));
    if let EmulationStrategy::PostProcessing { detail } = &s {
        assert!(detail.contains("stop sequence"));
    }
}

// ── Default strategy mapping for new capabilities ──────────────────────

#[test]
fn default_strategy_image_input_is_emulatable() {
    let s = default_strategy(&Capability::ImageInput);
    assert!(!matches!(s, EmulationStrategy::Disabled { .. }));
}

#[test]
fn default_strategy_stop_sequences_is_emulatable() {
    let s = default_strategy(&Capability::StopSequences);
    assert!(!matches!(s, EmulationStrategy::Disabled { .. }));
}

#[test]
fn can_emulate_image_input() {
    assert!(can_emulate(&Capability::ImageInput));
}

#[test]
fn can_emulate_stop_sequences() {
    assert!(can_emulate(&Capability::StopSequences));
}

// ── EmulationReport accuracy ───────────────────────────────────────────

#[test]
fn report_reflects_applied_strategies() {
    let mut conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "Base"))
        .push(IrMessage::text(IrRole::User, "Hi"));

    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[Capability::ExtendedThinking, Capability::ImageInput],
        &mut conv,
    );

    assert_eq!(report.applied.len(), 2);
    assert_eq!(report.applied[0].capability, Capability::ExtendedThinking);
    assert_eq!(report.applied[1].capability, Capability::ImageInput);
    assert!(report.warnings.is_empty());
}

#[test]
fn report_records_disabled_as_warnings() {
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Hi"));

    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::Streaming], &mut conv);

    assert!(report.applied.is_empty());
    assert_eq!(report.warnings.len(), 1);
    assert!(report.warnings[0].contains("Streaming"));
}

#[test]
fn report_entry_strategy_matches_resolved() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Hi"));

    let report = engine.apply(&[Capability::StopSequences], &mut conv);
    assert_eq!(report.applied.len(), 1);

    let resolved = engine.resolve_strategy(&Capability::StopSequences);
    assert_eq!(report.applied[0].strategy, resolved);
}

// ── Engine applies correct strategy per capability ─────────────────────

#[test]
fn engine_applies_system_prompt_for_image_input() {
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Describe this image"));

    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::ImageInput], &mut conv);

    assert_eq!(report.applied.len(), 1);
    assert!(matches!(
        report.applied[0].strategy,
        EmulationStrategy::SystemPromptInjection { .. }
    ));
    assert!(conv.system_message().is_some());
    let text = conv.system_message().unwrap().text_content();
    assert!(text.contains("Image"));
}

#[test]
fn engine_applies_post_processing_for_stop_sequences() {
    let original = IrConversation::new().push(IrMessage::text(IrRole::User, "Hello"));
    let mut conv = original.clone();

    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::StopSequences], &mut conv);

    assert_eq!(report.applied.len(), 1);
    assert!(matches!(
        report.applied[0].strategy,
        EmulationStrategy::PostProcessing { .. }
    ));
    // PostProcessing does not mutate the conversation
    assert_eq!(conv, original);
}

#[test]
fn engine_applies_extended_thinking_default() {
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Why?"));

    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);

    assert_eq!(report.applied.len(), 1);
    assert!(matches!(
        report.applied[0].strategy,
        EmulationStrategy::SystemPromptInjection { .. }
    ));
    let text = conv.system_message().unwrap().text_content();
    assert!(text.contains("Think step by step"));
}

// ── Composability: multiple emulations in one request ──────────────────

#[test]
fn multiple_system_prompt_injections_compose() {
    let mut conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are helpful."))
        .push(IrMessage::text(IrRole::User, "Complex task"));

    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[Capability::ExtendedThinking, Capability::ImageInput],
        &mut conv,
    );

    assert_eq!(report.applied.len(), 2);
    let sys_text = conv.system_message().unwrap().text_content();
    assert!(sys_text.contains("Think step by step"));
    assert!(sys_text.contains("Image"));
    assert!(sys_text.contains("You are helpful."));
}

#[test]
fn mixed_strategy_types_compose() {
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Do everything"));

    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[
            Capability::ExtendedThinking,           // SystemPromptInjection
            Capability::StopSequences,              // PostProcessing
            Capability::StructuredOutputJsonSchema, // PostProcessing
        ],
        &mut conv,
    );

    assert_eq!(report.applied.len(), 3);
    assert!(report.warnings.is_empty());
}

#[test]
fn composing_emulated_and_disabled() {
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Mix"));

    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[
            Capability::ExtendedThinking,
            Capability::Streaming, // disabled
            Capability::ImageInput,
        ],
        &mut conv,
    );

    assert_eq!(report.applied.len(), 2);
    assert_eq!(report.warnings.len(), 1);
}

// ── Fidelity labels ────────────────────────────────────────────────────

#[test]
fn fidelity_labels_native_capabilities() {
    let report = EmulationReport::default();
    let labels = compute_fidelity(&[Capability::Streaming, Capability::ToolUse], &report);

    assert_eq!(labels.len(), 2);
    assert_eq!(labels[&Capability::Streaming], FidelityLabel::Native);
    assert_eq!(labels[&Capability::ToolUse], FidelityLabel::Native);
}

#[test]
fn fidelity_labels_emulated_capabilities() {
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Hi"));

    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);

    let labels = compute_fidelity(&[], &report);

    assert_eq!(labels.len(), 1);
    assert!(matches!(
        labels[&Capability::ExtendedThinking],
        FidelityLabel::Emulated { .. }
    ));
}

#[test]
fn fidelity_labels_mixed_native_and_emulated() {
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Hi"));

    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::ImageInput], &mut conv);

    let labels = compute_fidelity(&[Capability::Streaming, Capability::ToolUse], &report);

    assert_eq!(labels.len(), 3);
    assert_eq!(labels[&Capability::Streaming], FidelityLabel::Native);
    assert_eq!(labels[&Capability::ToolUse], FidelityLabel::Native);
    assert!(matches!(
        labels[&Capability::ImageInput],
        FidelityLabel::Emulated { .. }
    ));
}

#[test]
fn fidelity_labels_empty_inputs() {
    let report = EmulationReport::default();
    let labels = compute_fidelity(&[], &report);
    assert!(labels.is_empty());
}

#[test]
fn fidelity_emulated_entry_carries_strategy() {
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Hi"));

    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::StopSequences], &mut conv);

    let labels = compute_fidelity(&[], &report);
    if let FidelityLabel::Emulated { strategy } = &labels[&Capability::StopSequences] {
        assert!(matches!(strategy, EmulationStrategy::PostProcessing { .. }));
    } else {
        panic!("expected Emulated fidelity label");
    }
}

// ── Strategy selection via config overrides ─────────────────────────────

#[test]
fn config_override_selects_custom_strategy() {
    let mut config = EmulationConfig::new();
    config.set(Capability::CodeExecution, emulate_code_execution());

    let engine = EmulationEngine::new(config);
    let strategy = engine.resolve_strategy(&Capability::CodeExecution);

    assert!(matches!(
        strategy,
        EmulationStrategy::SystemPromptInjection { .. }
    ));
}

#[test]
fn config_override_enables_normally_disabled_capability() {
    let mut config = EmulationConfig::new();
    config.set(Capability::CodeExecution, emulate_code_execution());

    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Run code"));

    let engine = EmulationEngine::new(config);
    let report = engine.apply(&[Capability::CodeExecution], &mut conv);

    assert_eq!(report.applied.len(), 1);
    assert!(report.warnings.is_empty());
    assert!(conv.system_message().is_some());
}

#[test]
fn config_override_with_structured_output_strategy() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::StructuredOutputJsonSchema,
        emulate_structured_output(),
    );

    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Give JSON"));

    let engine = EmulationEngine::new(config);
    let report = engine.apply(&[Capability::StructuredOutputJsonSchema], &mut conv);

    assert_eq!(report.applied.len(), 1);
    assert!(matches!(
        report.applied[0].strategy,
        EmulationStrategy::SystemPromptInjection { .. }
    ));
    let sys_text = conv.system_message().unwrap().text_content();
    assert!(sys_text.contains("JSON"));
}

#[test]
fn config_override_can_disable_normally_emulatable() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ImageInput,
        EmulationStrategy::Disabled {
            reason: "policy restriction".into(),
        },
    );

    let engine = EmulationEngine::new(config);
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Image"));
    let report = engine.apply(&[Capability::ImageInput], &mut conv);

    assert!(report.applied.is_empty());
    assert_eq!(report.warnings.len(), 1);
}

// ── Edge cases ─────────────────────────────────────────────────────────

#[test]
fn no_emulation_needed_empty_list() {
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Hi"));
    let original = conv.clone();

    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[], &mut conv);

    assert!(report.is_empty());
    assert_eq!(conv, original);
}

#[test]
fn all_capabilities_emulated() {
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Everything"));

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
    assert!(!report.is_empty());
}

#[test]
fn all_capabilities_disabled() {
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Nothing works"));
    let original = conv.clone();

    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[
            Capability::Streaming,
            Capability::ToolUse,
            Capability::CodeExecution,
        ],
        &mut conv,
    );

    assert!(report.applied.is_empty());
    assert_eq!(report.warnings.len(), 3);
    assert!(report.has_unemulatable());
    assert_eq!(conv, original);
}

#[test]
fn partially_emulated_report() {
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Partial"));

    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[
            Capability::ExtendedThinking, // emulated
            Capability::Streaming,        // disabled
        ],
        &mut conv,
    );

    assert_eq!(report.applied.len(), 1);
    assert_eq!(report.warnings.len(), 1);
    assert!(!report.is_empty());
    assert!(report.has_unemulatable());
}

#[test]
fn check_missing_without_mutation() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[Capability::ExtendedThinking, Capability::CodeExecution]);

    assert_eq!(report.applied.len(), 1);
    assert_eq!(report.warnings.len(), 1);
}

#[test]
fn check_missing_matches_apply_report() {
    let engine = EmulationEngine::with_defaults();
    let caps = [Capability::ImageInput, Capability::Streaming];

    let check_report = engine.check_missing(&caps);
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Hi"));
    let apply_report = engine.apply(&caps, &mut conv);

    assert_eq!(check_report.applied.len(), apply_report.applied.len());
    assert_eq!(check_report.warnings.len(), apply_report.warnings.len());
}

// ── Serde round-trips for new types ────────────────────────────────────

#[test]
fn fidelity_label_serde_roundtrip() {
    let labels = vec![
        FidelityLabel::Native,
        FidelityLabel::Emulated {
            strategy: EmulationStrategy::SystemPromptInjection {
                prompt: "test".into(),
            },
        },
        FidelityLabel::Emulated {
            strategy: EmulationStrategy::PostProcessing {
                detail: "truncate".into(),
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

#[test]
fn compute_fidelity_serde_roundtrip() {
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Hi"));
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::ImageInput], &mut conv);
    let labels = compute_fidelity(&[Capability::Streaming], &report);

    let json = serde_json::to_string(&labels).unwrap();
    let decoded: std::collections::BTreeMap<Capability, FidelityLabel> =
        serde_json::from_str(&json).unwrap();
    assert_eq!(labels, decoded);
}

// ════════════════════════════════════════════════════════════════════════
// Thinking Emulation Tests
// ════════════════════════════════════════════════════════════════════════

#[test]
fn thinking_brief_prompt_contains_step_by_step() {
    let emu = ThinkingEmulation::brief();
    assert!(emu.prompt_text().contains("step by step"));
}

#[test]
fn thinking_standard_prompt_contains_thinking_tags() {
    let emu = ThinkingEmulation::standard();
    let text = emu.prompt_text();
    assert!(text.contains("<thinking>"));
    assert!(text.contains("</thinking>"));
}

#[test]
fn thinking_detailed_prompt_contains_verification() {
    let emu = ThinkingEmulation::detailed();
    assert!(emu.prompt_text().contains("Verify"));
}

#[test]
fn thinking_detailed_prompt_contains_sub_problems() {
    let emu = ThinkingEmulation::detailed();
    assert!(emu.prompt_text().contains("sub-problems"));
}

#[test]
fn thinking_inject_appends_to_existing_system() {
    let emu = ThinkingEmulation::standard();
    let mut conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are helpful."))
        .push(IrMessage::text(IrRole::User, "Hello"));

    emu.inject(&mut conv);

    let sys = conv.system_message().unwrap();
    assert!(sys.text_content().contains("You are helpful."));
    assert!(sys.text_content().contains("<thinking>"));
}

#[test]
fn thinking_inject_creates_system_if_missing() {
    let emu = ThinkingEmulation::brief();
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Hi"));

    emu.inject(&mut conv);

    assert_eq!(conv.messages[0].role, IrRole::System);
    assert!(conv.messages[0].text_content().contains("step by step"));
}

#[test]
fn thinking_extract_with_tags() {
    let text = "Some preamble <thinking>I need to think about this</thinking>The answer is 42.";
    let (thinking, answer) = ThinkingEmulation::extract_thinking(text);
    assert_eq!(thinking, "I need to think about this");
    assert!(answer.contains("42"));
}

#[test]
fn thinking_extract_without_tags() {
    let text = "Just a plain answer without thinking.";
    let (thinking, answer) = ThinkingEmulation::extract_thinking(text);
    assert!(thinking.is_empty());
    assert_eq!(answer, text);
}

#[test]
fn thinking_extract_empty_tags() {
    let text = "<thinking></thinking>The answer.";
    let (thinking, answer) = ThinkingEmulation::extract_thinking(text);
    assert!(thinking.is_empty());
    assert!(answer.contains("The answer."));
}

#[test]
fn thinking_extract_multiline() {
    let text = "<thinking>\nStep 1: analyze\nStep 2: solve\n</thinking>\nFinal answer.";
    let (thinking, answer) = ThinkingEmulation::extract_thinking(text);
    assert!(thinking.contains("Step 1"));
    assert!(thinking.contains("Step 2"));
    assert!(answer.contains("Final answer."));
}

#[test]
fn thinking_extract_only_tags_no_answer() {
    let text = "<thinking>Some reasoning</thinking>";
    let (thinking, answer) = ThinkingEmulation::extract_thinking(text);
    assert_eq!(thinking, "Some reasoning");
    assert!(answer.is_empty());
}

#[test]
fn thinking_extract_preamble_before_tags() {
    let text = "Let me think. <thinking>reasoning</thinking> Done.";
    let (thinking, answer) = ThinkingEmulation::extract_thinking(text);
    assert_eq!(thinking, "reasoning");
    assert!(answer.contains("Let me think."));
    assert!(answer.contains("Done."));
}

#[test]
fn thinking_to_thinking_block_some() {
    let text = "<thinking>Step 1: analyze</thinking>Answer.";
    let block = ThinkingEmulation::to_thinking_block(text);
    assert!(block.is_some());
    if let Some(IrContentBlock::Thinking { text: t }) = block {
        assert_eq!(t, "Step 1: analyze");
    }
}

#[test]
fn thinking_to_thinking_block_none() {
    let text = "No thinking tags here.";
    let block = ThinkingEmulation::to_thinking_block(text);
    assert!(block.is_none());
}

#[test]
fn thinking_detail_levels_differ() {
    let brief = ThinkingEmulation::brief().prompt_text();
    let standard = ThinkingEmulation::standard().prompt_text();
    let detailed = ThinkingEmulation::detailed().prompt_text();
    assert_ne!(brief, standard);
    assert_ne!(standard, detailed);
    assert_ne!(brief, detailed);
}

#[test]
fn thinking_inject_preserves_user_message() {
    let emu = ThinkingEmulation::standard();
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Solve 2+2"));
    emu.inject(&mut conv);

    assert_eq!(conv.messages.len(), 2);
    assert_eq!(conv.messages[1].text_content(), "Solve 2+2");
}

// ════════════════════════════════════════════════════════════════════════
// Tool Use Emulation Tests
// ════════════════════════════════════════════════════════════════════════

fn sample_tools() -> Vec<IrToolDefinition> {
    vec![
        IrToolDefinition {
            name: "read_file".into(),
            description: "Read a file from disk".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"}
                }
            }),
        },
        IrToolDefinition {
            name: "write_file".into(),
            description: "Write content to a file".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "content": {"type": "string"}
                }
            }),
        },
    ]
}

#[test]
fn tool_prompt_empty_tools() {
    let prompt = ToolUseEmulation::tools_to_prompt(&[]);
    assert!(prompt.is_empty());
}

#[test]
fn tool_prompt_contains_tool_names() {
    let prompt = ToolUseEmulation::tools_to_prompt(&sample_tools());
    assert!(prompt.contains("read_file"));
    assert!(prompt.contains("write_file"));
}

#[test]
fn tool_prompt_contains_descriptions() {
    let prompt = ToolUseEmulation::tools_to_prompt(&sample_tools());
    assert!(prompt.contains("Read a file"));
    assert!(prompt.contains("Write content"));
}

#[test]
fn tool_prompt_contains_instructions() {
    let prompt = ToolUseEmulation::tools_to_prompt(&sample_tools());
    assert!(prompt.contains("<tool_call>"));
    assert!(prompt.contains("</tool_call>"));
}

#[test]
fn tool_prompt_contains_parameters() {
    let prompt = ToolUseEmulation::tools_to_prompt(&sample_tools());
    assert!(prompt.contains("path"));
    assert!(prompt.contains("content"));
}

#[test]
fn tool_inject_creates_system_message() {
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Read my file"));

    ToolUseEmulation::inject_tools(&mut conv, &sample_tools());

    assert_eq!(conv.messages[0].role, IrRole::System);
    assert!(conv.messages[0].text_content().contains("read_file"));
}

#[test]
fn tool_inject_appends_to_existing_system() {
    let mut conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "Be helpful."))
        .push(IrMessage::text(IrRole::User, "Hi"));

    ToolUseEmulation::inject_tools(&mut conv, &sample_tools());

    let sys = conv.system_message().unwrap();
    assert!(sys.text_content().contains("Be helpful."));
    assert!(sys.text_content().contains("read_file"));
}

#[test]
fn tool_inject_noop_for_empty_tools() {
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Hi"));
    let original = conv.clone();

    ToolUseEmulation::inject_tools(&mut conv, &[]);
    assert_eq!(conv, original);
}

#[test]
fn tool_parse_single_call() {
    let text = r#"I'll read the file.
<tool_call>
{"name": "read_file", "arguments": {"path": "test.txt"}}
</tool_call>"#;

    let calls = ToolUseEmulation::parse_tool_calls(text);
    assert_eq!(calls.len(), 1);
    let call = calls[0].as_ref().unwrap();
    assert_eq!(call.name, "read_file");
    assert_eq!(call.arguments["path"], "test.txt");
}

#[test]
fn tool_parse_multiple_calls() {
    let text = r#"<tool_call>
{"name": "read_file", "arguments": {"path": "a.txt"}}
</tool_call>
Then
<tool_call>
{"name": "write_file", "arguments": {"path": "b.txt", "content": "hello"}}
</tool_call>"#;

    let calls = ToolUseEmulation::parse_tool_calls(text);
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].as_ref().unwrap().name, "read_file");
    assert_eq!(calls[1].as_ref().unwrap().name, "write_file");
}

#[test]
fn tool_parse_no_calls() {
    let text = "I don't need any tools to answer this question.";
    let calls = ToolUseEmulation::parse_tool_calls(text);
    assert!(calls.is_empty());
}

#[test]
fn tool_parse_invalid_json() {
    let text = "<tool_call>\nnot valid json\n</tool_call>";
    let calls = ToolUseEmulation::parse_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert!(calls[0].is_err());
    assert!(calls[0].as_ref().unwrap_err().contains("invalid JSON"));
}

#[test]
fn tool_parse_missing_name() {
    let text = r#"<tool_call>
{"arguments": {"path": "test.txt"}}
</tool_call>"#;
    let calls = ToolUseEmulation::parse_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert!(calls[0].is_err());
    assert!(calls[0].as_ref().unwrap_err().contains("missing 'name'"));
}

#[test]
fn tool_parse_missing_arguments_defaults_to_null() {
    let text = r#"<tool_call>
{"name": "list_files"}
</tool_call>"#;
    let calls = ToolUseEmulation::parse_tool_calls(text);
    assert_eq!(calls.len(), 1);
    let call = calls[0].as_ref().unwrap();
    assert_eq!(call.name, "list_files");
    assert!(call.arguments.is_null());
}

#[test]
fn tool_parse_unclosed_tag() {
    let text = "Some text <tool_call>\n{\"name\": \"foo\"}\nno closing tag";
    let calls = ToolUseEmulation::parse_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert!(calls[0].is_err());
    assert!(calls[0].as_ref().unwrap_err().contains("unclosed"));
}

#[test]
fn tool_to_tool_use_block() {
    let call = ParsedToolCall {
        name: "read_file".into(),
        arguments: serde_json::json!({"path": "a.txt"}),
    };
    let block = ToolUseEmulation::to_tool_use_block(&call, "tc-001");

    if let IrContentBlock::ToolUse { id, name, input } = block {
        assert_eq!(id, "tc-001");
        assert_eq!(name, "read_file");
        assert_eq!(input["path"], "a.txt");
    } else {
        panic!("expected ToolUse block");
    }
}

#[test]
fn tool_format_result_success() {
    let result = ToolUseEmulation::format_tool_result("read_file", "file contents here", false);
    assert!(result.contains("read_file"));
    assert!(result.contains("file contents here"));
    assert!(!result.contains("error"));
}

#[test]
fn tool_format_result_error() {
    let result = ToolUseEmulation::format_tool_result("write_file", "permission denied", true);
    assert!(result.contains("error"));
    assert!(result.contains("permission denied"));
}

#[test]
fn tool_extract_text_outside_calls() {
    let text = "Preamble. <tool_call>\n{\"name\":\"f\"}\n</tool_call> Epilogue.";
    let outside = ToolUseEmulation::extract_text_outside_tool_calls(text);
    assert!(outside.contains("Preamble."));
    assert!(outside.contains("Epilogue."));
    assert!(!outside.contains("tool_call"));
}

#[test]
fn tool_extract_text_no_calls() {
    let text = "Just regular text.";
    let outside = ToolUseEmulation::extract_text_outside_tool_calls(text);
    assert_eq!(outside, text);
}

#[test]
fn tool_extract_text_multiple_calls() {
    let text = "A <tool_call>{}</tool_call> B <tool_call>{}</tool_call> C";
    let outside = ToolUseEmulation::extract_text_outside_tool_calls(text);
    assert!(outside.contains('A'));
    assert!(outside.contains('B'));
    assert!(outside.contains('C'));
}

#[test]
fn tool_parse_with_whitespace_around_json() {
    let text = "<tool_call>\n\n  {\"name\": \"f\", \"arguments\": {}}  \n\n</tool_call>";
    let calls = ToolUseEmulation::parse_tool_calls(text);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].as_ref().unwrap().name, "f");
}

// ════════════════════════════════════════════════════════════════════════
// Vision Emulation Tests
// ════════════════════════════════════════════════════════════════════════

fn conv_with_image() -> IrConversation {
    IrConversation::new().push(IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text {
                text: "What is this?".into(),
            },
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "base64data".into(),
            },
        ],
    ))
}

#[test]
fn vision_has_images_true() {
    let conv = conv_with_image();
    assert!(VisionEmulation::has_images(&conv));
}

#[test]
fn vision_has_images_false() {
    let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "No images"));
    assert!(!VisionEmulation::has_images(&conv));
}

#[test]
fn vision_replace_returns_count() {
    let mut conv = conv_with_image();
    let count = VisionEmulation::replace_images_with_placeholders(&mut conv);
    assert_eq!(count, 1);
}

#[test]
fn vision_replace_substitutes_image_blocks() {
    let mut conv = conv_with_image();
    VisionEmulation::replace_images_with_placeholders(&mut conv);

    let user = &conv.messages[0];
    assert_eq!(user.content.len(), 2);
    // Second block should now be text, not image
    assert!(matches!(user.content[1], IrContentBlock::Text { .. }));
    assert!(user.text_content().contains("image/png"));
}

#[test]
fn vision_replace_preserves_text_blocks() {
    let mut conv = conv_with_image();
    VisionEmulation::replace_images_with_placeholders(&mut conv);

    if let IrContentBlock::Text { text } = &conv.messages[0].content[0] {
        assert_eq!(text, "What is this?");
    } else {
        panic!("expected Text block");
    }
}

#[test]
fn vision_replace_no_images_returns_zero() {
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Hi"));
    let count = VisionEmulation::replace_images_with_placeholders(&mut conv);
    assert_eq!(count, 0);
}

#[test]
fn vision_replace_multiple_images() {
    let mut conv = IrConversation::new().push(IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "img1".into(),
            },
            IrContentBlock::Image {
                media_type: "image/jpeg".into(),
                data: "img2".into(),
            },
        ],
    ));
    let count = VisionEmulation::replace_images_with_placeholders(&mut conv);
    assert_eq!(count, 2);
    assert!(conv.messages[0].text_content().contains("image/jpeg"));
}

#[test]
fn vision_fallback_prompt_injected() {
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Hi"));
    VisionEmulation::inject_vision_fallback_prompt(&mut conv, 3);

    assert_eq!(conv.messages[0].role, IrRole::System);
    assert!(conv.messages[0].text_content().contains("3 image(s)"));
}

#[test]
fn vision_fallback_prompt_noop_for_zero() {
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Hi"));
    let original = conv.clone();
    VisionEmulation::inject_vision_fallback_prompt(&mut conv, 0);
    assert_eq!(conv, original);
}

#[test]
fn vision_apply_full_pipeline() {
    let mut conv = conv_with_image();
    let count = VisionEmulation::apply(&mut conv);

    assert_eq!(count, 1);
    // System message was injected
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert!(conv.messages[0].text_content().contains("1 image(s)"));
    // Image was replaced
    assert!(!VisionEmulation::has_images(&conv));
}

#[test]
fn vision_apply_no_images_noop() {
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Hi"));
    let original = conv.clone();
    let count = VisionEmulation::apply(&mut conv);
    assert_eq!(count, 0);
    assert_eq!(conv, original);
}

#[test]
fn vision_images_across_multiple_messages() {
    let mut conv = IrConversation::new()
        .push(IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "a".into(),
            }],
        ))
        .push(IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Image {
                media_type: "image/gif".into(),
                data: "b".into(),
            }],
        ));
    let count = VisionEmulation::apply(&mut conv);
    assert_eq!(count, 2);
    assert!(!VisionEmulation::has_images(&conv));
}

// ════════════════════════════════════════════════════════════════════════
// Streaming Emulation Tests
// ════════════════════════════════════════════════════════════════════════

#[test]
fn streaming_empty_text_single_chunk() {
    let emu = StreamingEmulation::default_chunk_size();
    let chunks = emu.split_into_chunks("");
    assert_eq!(chunks.len(), 1);
    assert!(chunks[0].content.is_empty());
    assert!(chunks[0].is_final);
}

#[test]
fn streaming_short_text_single_chunk() {
    let emu = StreamingEmulation::new(50);
    let chunks = emu.split_into_chunks("Hello world");
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].content, "Hello world");
    assert!(chunks[0].is_final);
    assert_eq!(chunks[0].index, 0);
}

#[test]
fn streaming_split_multiple_chunks() {
    let emu = StreamingEmulation::new(5);
    let chunks = emu.split_into_chunks("Hello world, how are you?");
    assert!(chunks.len() > 1);
    assert!(chunks.last().unwrap().is_final);
}

#[test]
fn streaming_reassemble_roundtrip() {
    let emu = StreamingEmulation::new(10);
    let text = "This is a test of the streaming emulation system.";
    let chunks = emu.split_into_chunks(text);
    let reassembled = StreamingEmulation::reassemble(&chunks);
    assert_eq!(reassembled, text);
}

#[test]
fn streaming_fixed_split_roundtrip() {
    let emu = StreamingEmulation::new(7);
    let text = "abcdefghijklmnopqrstuvwxyz";
    let chunks = emu.split_fixed(text);
    let reassembled = StreamingEmulation::reassemble(&chunks);
    assert_eq!(reassembled, text);
}

#[test]
fn streaming_fixed_chunk_sizes() {
    let emu = StreamingEmulation::new(5);
    let chunks = emu.split_fixed("abcdefghijklm");
    // 13 chars / 5 = 3 full chunks (5,5,3)
    assert_eq!(chunks.len(), 3);
    assert_eq!(chunks[0].content, "abcde");
    assert_eq!(chunks[1].content, "fghij");
    assert_eq!(chunks[2].content, "klm");
}

#[test]
fn streaming_indices_sequential() {
    let emu = StreamingEmulation::new(5);
    let chunks = emu.split_into_chunks("Hello world, how are you?");
    for (i, chunk) in chunks.iter().enumerate() {
        assert_eq!(chunk.index, i);
    }
}

#[test]
fn streaming_only_last_is_final() {
    let emu = StreamingEmulation::new(5);
    let chunks = emu.split_into_chunks("Hello world, how are you?");
    for chunk in &chunks[..chunks.len() - 1] {
        assert!(!chunk.is_final);
    }
    assert!(chunks.last().unwrap().is_final);
}

#[test]
fn streaming_chunk_size_one() {
    let emu = StreamingEmulation::new(1);
    let chunks = emu.split_fixed("abc");
    assert_eq!(chunks.len(), 3);
    assert_eq!(chunks[0].content, "a");
    assert_eq!(chunks[1].content, "b");
    assert_eq!(chunks[2].content, "c");
}

#[test]
fn streaming_word_boundary_preference() {
    let emu = StreamingEmulation::new(10);
    let chunks = emu.split_into_chunks("Hello world foo bar");
    // First chunk should break at word boundary
    assert!(
        chunks[0].content.ends_with(' ') || chunks[0].content.len() <= 10,
        "chunk should prefer word boundaries"
    );
}

#[test]
fn streaming_fixed_empty_text() {
    let emu = StreamingEmulation::new(5);
    let chunks = emu.split_fixed("");
    assert_eq!(chunks.len(), 1);
    assert!(chunks[0].content.is_empty());
    assert!(chunks[0].is_final);
}

#[test]
fn streaming_default_chunk_size_is_20() {
    let emu = StreamingEmulation::default_chunk_size();
    assert_eq!(emu.chunk_size(), 20);
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

#[test]
fn streaming_chunk_minimum_size() {
    let emu = StreamingEmulation::new(0);
    assert_eq!(emu.chunk_size(), 1);
}

// ════════════════════════════════════════════════════════════════════════
// Strategy Selection / Integration Tests
// ════════════════════════════════════════════════════════════════════════

#[test]
fn strategy_thinking_integrates_with_engine() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::SystemPromptInjection {
            prompt: ThinkingEmulation::standard().prompt_text().into(),
        },
    );

    let engine = EmulationEngine::new(config);
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Solve x^2=4"));
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);

    assert_eq!(report.applied.len(), 1);
    let sys = conv.system_message().unwrap().text_content();
    assert!(sys.contains("<thinking>"));
}

#[test]
fn strategy_tool_use_integrates_with_engine() {
    let prompt = ToolUseEmulation::tools_to_prompt(&sample_tools());
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ToolUse,
        EmulationStrategy::SystemPromptInjection { prompt },
    );

    let engine = EmulationEngine::new(config);
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Read file"));
    let report = engine.apply(&[Capability::ToolUse], &mut conv);

    assert_eq!(report.applied.len(), 1);
    assert!(report.warnings.is_empty());
    assert!(
        conv.system_message()
            .unwrap()
            .text_content()
            .contains("read_file")
    );
}

#[test]
fn strategy_disabled_tool_use_by_default() {
    let engine = EmulationEngine::with_defaults();
    let s = engine.resolve_strategy(&Capability::ToolUse);
    assert!(matches!(s, EmulationStrategy::Disabled { .. }));
}

#[test]
fn strategy_disabled_streaming_by_default() {
    let engine = EmulationEngine::with_defaults();
    let s = engine.resolve_strategy(&Capability::Streaming);
    assert!(matches!(s, EmulationStrategy::Disabled { .. }));
}

#[test]
fn combined_thinking_and_vision_emulation() {
    let emu_thinking = ThinkingEmulation::standard();
    let mut conv = conv_with_image();

    // Apply vision first, then thinking
    VisionEmulation::apply(&mut conv);
    emu_thinking.inject(&mut conv);

    let sys = conv.system_message().unwrap().text_content();
    assert!(sys.contains("image(s)"));
    assert!(sys.contains("<thinking>"));
    assert!(!VisionEmulation::has_images(&conv));
}

#[test]
fn tool_null_parameters_omitted_from_prompt() {
    let tools = vec![IrToolDefinition {
        name: "noop".into(),
        description: "Does nothing".into(),
        parameters: serde_json::Value::Null,
    }];
    let prompt = ToolUseEmulation::tools_to_prompt(&tools);
    assert!(prompt.contains("noop"));
    assert!(!prompt.contains("Parameters:"));
}

#[test]
fn parsed_tool_call_serde_roundtrip() {
    let call = ParsedToolCall {
        name: "test".into(),
        arguments: serde_json::json!({"key": "value"}),
    };
    let json = serde_json::to_string(&call).unwrap();
    let decoded: ParsedToolCall = serde_json::from_str(&json).unwrap();
    assert_eq!(call, decoded);
}

#[test]
fn thinking_detail_serde_roundtrip() {
    let details = [
        ThinkingDetail::Brief,
        ThinkingDetail::Standard,
        ThinkingDetail::Detailed,
    ];
    for d in &details {
        let json = serde_json::to_string(d).unwrap();
        let decoded: ThinkingDetail = serde_json::from_str(&json).unwrap();
        assert_eq!(*d, decoded);
    }
}
