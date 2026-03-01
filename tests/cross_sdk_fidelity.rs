// SPDX-License-Identifier: MIT OR Apache-2.0
//! Cross-SDK fidelity tests verifying mapping correctness between all dialect pairs.
//!
//! Covers message role mapping, tool definition translation, streaming event parity,
//! capability fidelity, and edge cases across the full projection matrix.

use abp_core::{
    AgentEvent, AgentEventKind, Capability, CapabilityManifest, SupportLevel, WorkOrderBuilder,
};
use abp_integrations::projection::{
    Dialect, Message, MessageRole, ProjectionMatrix, ToolCall, ToolDefinitionIr, ToolResult,
    TranslationFidelity, translate,
};
use chrono::Utc;
use serde_json::json;
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn matrix() -> ProjectionMatrix {
    ProjectionMatrix::new()
}

/// All seven registered string dialect names.
const ALL_STRING_DIALECTS: &[&str] = &[
    "abp",
    "openai",
    "anthropic",
    "gemini",
    "codex",
    "kimi",
    "mock",
];

/// Vendor (non-ABP, non-Mock) Dialect enum variants.
const VENDOR_DIALECTS: &[Dialect] = &[
    Dialect::Claude,
    Dialect::Codex,
    Dialect::Gemini,
    Dialect::Kimi,
    Dialect::OpenAi,
];

fn all_sdk_manifests() -> Vec<(&'static str, CapabilityManifest)> {
    vec![
        ("claude", abp_claude_sdk::dialect::capability_manifest()),
        ("codex", abp_codex_sdk::dialect::capability_manifest()),
        ("gemini", abp_gemini_sdk::dialect::capability_manifest()),
        ("kimi", abp_kimi_sdk::dialect::capability_manifest()),
        ("openai", abp_openai_sdk::dialect::capability_manifest()),
    ]
}

fn make_tool_def(name: &str, desc: &str, params: serde_json::Value) -> ToolDefinitionIr {
    ToolDefinitionIr {
        name: name.to_string(),
        description: desc.to_string(),
        parameters: params,
    }
}

// =========================================================================
// 1. Message Role Mapping Fidelity (8 tests)
// =========================================================================

#[test]
fn role_system_to_system_openai_codex_identity() {
    // OpenAI and Codex use the same format — system stays system.
    let m = matrix();
    let msgs = vec![Message {
        role: MessageRole::System,
        content: "You are a helpful assistant.".into(),
    }];
    let mapped_openai = m
        .map_messages(Dialect::Abp, Dialect::OpenAi, &msgs)
        .unwrap();
    assert_eq!(mapped_openai[0].role, MessageRole::System);
    assert_eq!(mapped_openai[0].content, "You are a helpful assistant.");

    let mapped_codex = m.map_messages(Dialect::Abp, Dialect::Codex, &msgs).unwrap();
    assert_eq!(mapped_codex[0].role, MessageRole::System);
    assert_eq!(mapped_codex[0].content, "You are a helpful assistant.");
}

#[test]
fn role_system_to_user_with_prefix_claude() {
    // Claude wraps system messages as user messages with [System] prefix.
    let m = matrix();
    let msgs = vec![Message {
        role: MessageRole::System,
        content: "Be concise.".into(),
    }];
    let mapped = m
        .map_messages(Dialect::Abp, Dialect::Claude, &msgs)
        .unwrap();
    assert_eq!(mapped[0].role, MessageRole::User);
    assert!(mapped[0].content.starts_with("[System] "));
    assert!(mapped[0].content.contains("Be concise."));
}

#[test]
fn role_system_to_user_with_prefix_gemini() {
    // Gemini also lacks native system role, same behavior as Claude.
    let m = matrix();
    let msgs = vec![Message {
        role: MessageRole::System,
        content: "Answer briefly.".into(),
    }];
    let mapped = m
        .map_messages(Dialect::Abp, Dialect::Gemini, &msgs)
        .unwrap();
    assert_eq!(mapped[0].role, MessageRole::User);
    assert!(mapped[0].content.starts_with("[System] "));
    assert!(mapped[0].content.contains("Answer briefly."));
}

#[test]
fn role_user_to_user_all_pairs_lossless() {
    let m = matrix();
    let msgs = vec![Message {
        role: MessageRole::User,
        content: "Hello from the user".into(),
    }];
    for &from in Dialect::ALL {
        for &to in Dialect::ALL {
            let mapped = m.map_messages(from, to, &msgs).unwrap();
            assert_eq!(mapped[0].role, MessageRole::User);
            assert_eq!(
                mapped[0].content, "Hello from the user",
                "user role mapping failed for {from:?} -> {to:?}"
            );
        }
    }
}

#[test]
fn role_assistant_to_assistant_all_pairs_lossless() {
    let m = matrix();
    let msgs = vec![Message {
        role: MessageRole::Assistant,
        content: "I am the assistant.".into(),
    }];
    for &from in Dialect::ALL {
        for &to in Dialect::ALL {
            let mapped = m.map_messages(from, to, &msgs).unwrap();
            assert_eq!(mapped[0].role, MessageRole::Assistant);
            assert_eq!(
                mapped[0].content, "I am the assistant.",
                "assistant role mapping failed for {from:?} -> {to:?}"
            );
        }
    }
}

#[test]
fn role_tool_to_tool_openai_codex_same_format() {
    // OpenAI and Codex share tool call/result format — tool names map identically.
    let m = matrix();
    let call = ToolCall {
        tool_name: "file_read".to_string(),
        tool_use_id: Some("tc-1".into()),
        parent_tool_use_id: None,
        input: json!({"path": "main.rs"}),
    };
    // openai -> codex: codex uses same names as openai
    let translated = m.translate_tool_call("openai", "codex", &call).unwrap();
    assert_eq!(translated.tool_use_id, call.tool_use_id);
    assert_eq!(translated.input, call.input);
}

#[test]
fn role_tool_result_openai_to_anthropic_field_preservation() {
    // OpenAI tool_result maps to Anthropic — field names differ but content preserved.
    let m = matrix();
    let result = ToolResult {
        tool_name: "file_read".to_string(),
        tool_use_id: Some("tc-1".into()),
        output: json!({"content": "fn main() {}"}),
        is_error: false,
    };
    let translated = m
        .translate_tool_result("openai", "anthropic", &result)
        .unwrap();
    assert_eq!(translated.tool_name, "Read"); // mapped name
    assert_eq!(translated.tool_use_id, result.tool_use_id);
    assert_eq!(translated.output, result.output);
    assert_eq!(translated.is_error, result.is_error);
}

#[test]
fn multi_turn_conversation_preserved_through_mapping() {
    let m = matrix();
    let conversation = vec![
        Message {
            role: MessageRole::System,
            content: "You write Rust.".into(),
        },
        Message {
            role: MessageRole::User,
            content: "Write hello world.".into(),
        },
        Message {
            role: MessageRole::Assistant,
            content: "fn main() { println!(\"Hello\"); }".into(),
        },
        Message {
            role: MessageRole::User,
            content: "Add a test.".into(),
        },
        Message {
            role: MessageRole::Assistant,
            content: "#[test] fn it_works() {}".into(),
        },
    ];

    // OpenAI preserves all roles as-is.
    let openai_msgs = m
        .map_messages(Dialect::Abp, Dialect::OpenAi, &conversation)
        .unwrap();
    assert_eq!(openai_msgs.len(), 5);
    assert_eq!(openai_msgs[0].role, MessageRole::System);
    assert_eq!(openai_msgs[1].role, MessageRole::User);
    assert_eq!(openai_msgs[2].role, MessageRole::Assistant);
    assert_eq!(openai_msgs[3].role, MessageRole::User);
    assert_eq!(openai_msgs[4].role, MessageRole::Assistant);

    // Claude folds system into user but preserves sequence.
    let claude_msgs = m
        .map_messages(Dialect::Abp, Dialect::Claude, &conversation)
        .unwrap();
    assert_eq!(claude_msgs.len(), 5);
    assert_eq!(claude_msgs[0].role, MessageRole::User);
    assert!(claude_msgs[0].content.starts_with("[System] "));
    assert_eq!(claude_msgs[1].content, "Write hello world.");
    assert_eq!(claude_msgs[4].content, "#[test] fn it_works() {}");
}

// =========================================================================
// 2. Tool Definition Mapping (6 tests)
// =========================================================================

#[test]
fn tool_def_openai_function_to_claude_tool() {
    let m = matrix();
    let tool = make_tool_def(
        "file_read",
        "Read a file",
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File path"}
            },
            "required": ["path"]
        }),
    );
    let mapped = m
        .map_tool_definitions(
            Dialect::OpenAi,
            Dialect::Claude,
            std::slice::from_ref(&tool),
        )
        .unwrap();
    assert_eq!(mapped.len(), 1);
    assert_eq!(mapped[0].name, "Read"); // openai -> claude name
    assert_eq!(mapped[0].description, "Read a file");
    assert_eq!(mapped[0].parameters, tool.parameters); // JSON Schema preserved
}

#[test]
fn tool_def_openai_function_to_gemini_declaration() {
    let m = matrix();
    let tool = make_tool_def(
        "file_read",
        "Read file contents",
        json!({
            "type": "object",
            "properties": {
                "filePath": {"type": "string"}
            }
        }),
    );
    let mapped = m
        .map_tool_definitions(
            Dialect::OpenAi,
            Dialect::Gemini,
            std::slice::from_ref(&tool),
        )
        .unwrap();
    assert_eq!(mapped[0].name, "readFile"); // camelCase convention
    assert_eq!(mapped[0].parameters, tool.parameters);
}

#[test]
fn tool_def_claude_to_openai_reverse_mapping() {
    let m = matrix();
    let tool = make_tool_def(
        "Read",
        "Read a file from disk",
        json!({"type": "object", "properties": {"path": {"type": "string"}}}),
    );
    let mapped = m
        .map_tool_definitions(Dialect::Claude, Dialect::OpenAi, &[tool])
        .unwrap();
    assert_eq!(mapped[0].name, "file_read");
    assert_eq!(mapped[0].description, "Read a file from disk");
}

#[test]
fn tool_def_complex_nested_json_schema() {
    let m = matrix();
    let complex_params = json!({
        "type": "object",
        "properties": {
            "config": {
                "type": "object",
                "properties": {
                    "nested": {
                        "type": "object",
                        "properties": {
                            "deep_value": {"type": "integer", "minimum": 0},
                            "array_of_objects": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "key": {"type": "string"},
                                        "value": {"type": ["string", "number", "null"]}
                                    },
                                    "required": ["key"]
                                }
                            }
                        }
                    }
                }
            }
        },
        "required": ["config"]
    });
    let tool = make_tool_def("read_file", "Complex tool", complex_params.clone());
    // Translate through ABP -> each vendor and verify schema fidelity.
    for &target in VENDOR_DIALECTS {
        let mapped = m
            .map_tool_definitions(Dialect::Abp, target, std::slice::from_ref(&tool))
            .unwrap();
        assert_eq!(
            mapped[0].parameters, complex_params,
            "complex nested JSON Schema not preserved for {target:?}"
        );
    }
}

#[test]
fn tool_def_enum_parameters_preserved() {
    let m = matrix();
    let tool = make_tool_def(
        "bash",
        "Execute shell command",
        json!({
            "type": "object",
            "properties": {
                "shell": {
                    "type": "string",
                    "enum": ["bash", "zsh", "fish", "sh"]
                },
                "timeout_ms": {
                    "type": "integer",
                    "enum": [1000, 5000, 30000, 60000]
                }
            }
        }),
    );
    for &target in VENDOR_DIALECTS {
        let mapped = m
            .map_tool_definitions(Dialect::Abp, target, std::slice::from_ref(&tool))
            .unwrap();
        let shell_enum = &mapped[0].parameters["properties"]["shell"]["enum"];
        assert_eq!(shell_enum, &json!(["bash", "zsh", "fish", "sh"]));
        let timeout_enum = &mapped[0].parameters["properties"]["timeout_ms"]["enum"];
        assert_eq!(timeout_enum, &json!([1000, 5000, 30000, 60000]));
    }
}

#[test]
fn tool_def_no_parameters_empty_schema() {
    let m = matrix();
    let tool = make_tool_def("glob", "List files", json!({}));
    for &target in VENDOR_DIALECTS {
        let mapped = m
            .map_tool_definitions(Dialect::Abp, target, std::slice::from_ref(&tool))
            .unwrap();
        assert_eq!(
            mapped[0].parameters,
            json!({}),
            "empty schema should be preserved for {target:?}"
        );
    }
}

// =========================================================================
// 3. Streaming Event Parity (6 tests)
// =========================================================================

#[test]
fn text_delta_semantics_across_all_sdks() {
    let m = matrix();
    let delta = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta {
            text: "Hello, ".into(),
        },
        ext: None,
    };
    // All dialect pairs must preserve delta text content.
    for &from in ALL_STRING_DIALECTS {
        for &to in ALL_STRING_DIALECTS {
            let translated = m.translate_event(from, to, &delta).unwrap();
            match &translated.kind {
                AgentEventKind::AssistantDelta { text } => {
                    assert_eq!(text, "Hello, ", "delta text lost {from} -> {to}");
                }
                other => panic!("expected AssistantDelta, got {other:?}"),
            }
        }
    }
}

#[test]
fn tool_call_streaming_accumulation_openai_to_anthropic() {
    // OpenAI streams tool call fragments; Claude sends blocks.
    // Through the projection matrix, the ToolCall event is translated as a whole unit.
    let m = matrix();
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "file_read".into(),
            tool_use_id: Some("tc-stream-1".into()),
            parent_tool_use_id: None,
            input: json!({"path": "src/lib.rs", "encoding": "utf-8"}),
        },
        ext: None,
    };
    let translated = m.translate_event("openai", "anthropic", &event).unwrap();
    match &translated.kind {
        AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            input,
            ..
        } => {
            assert_eq!(tool_name, "Read");
            assert_eq!(tool_use_id.as_deref(), Some("tc-stream-1"));
            assert_eq!(input, &json!({"path": "src/lib.rs", "encoding": "utf-8"}));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn stop_reason_mapping_via_event_tables() {
    // Verify event mapping tables have run_completed equivalents across dialects.
    let m = matrix();
    let abp_to_openai = m.event_mapping("abp", "openai").unwrap();
    assert_eq!(
        abp_to_openai.kind_map.get("run_completed"),
        Some(&"response.completed".to_string())
    );

    let abp_to_anthropic = m.event_mapping("abp", "anthropic").unwrap();
    assert_eq!(
        abp_to_anthropic.kind_map.get("run_completed"),
        Some(&"message_stop".to_string())
    );

    let abp_to_gemini = m.event_mapping("abp", "gemini").unwrap();
    assert_eq!(
        abp_to_gemini.kind_map.get("run_completed"),
        Some(&"generate_content_end".to_string())
    );
}

#[test]
fn usage_token_counting_via_event_passthrough() {
    // Token usage metadata in ext is preserved through translation.
    let m = matrix();
    let mut ext = BTreeMap::new();
    ext.insert(
        "usage".to_string(),
        json!({
            "input_tokens": 150,
            "output_tokens": 42,
            "total_tokens": 192
        }),
    );
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunCompleted {
            message: "done".into(),
        },
        ext: Some(ext.clone()),
    };
    for &from in ALL_STRING_DIALECTS {
        for &to in ALL_STRING_DIALECTS {
            let translated = m.translate_event(from, to, &event).unwrap();
            let usage = translated.ext.as_ref().unwrap().get("usage").unwrap();
            assert_eq!(usage["input_tokens"], 150);
            assert_eq!(usage["output_tokens"], 42);
            assert_eq!(usage["total_tokens"], 192);
        }
    }
}

#[test]
fn finish_reason_variants_across_sdks() {
    // Each SDK uses different stop/finish terminology — verify event mapping tables.
    let m = matrix();
    // OpenAI: response.completed, Anthropic: message_stop, Gemini: generate_content_end
    let pairs: &[(&str, &str, &str)] = &[
        ("openai", "anthropic", "message_stop"),
        ("openai", "gemini", "generate_content_end"),
        ("anthropic", "openai", "response.completed"),
        ("anthropic", "gemini", "generate_content_end"),
        ("gemini", "openai", "response.completed"),
        ("gemini", "anthropic", "message_stop"),
    ];
    for &(from, to, expected_stop) in pairs {
        let mapping = m.event_mapping(from, to).unwrap();
        // Find the mapping entry for the source "completed" event.
        let source_completed = match from {
            "openai" => "response.completed",
            "anthropic" => "message_stop",
            "gemini" => "generate_content_end",
            _ => continue,
        };
        let target = mapping.kind_map.get(source_completed);
        assert_eq!(
            target,
            Some(&expected_stop.to_string()),
            "finish reason mapping {from} -> {to}"
        );
    }
}

#[test]
fn empty_delta_handling() {
    let m = matrix();
    let delta = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: "".into() },
        ext: None,
    };
    for &from in ALL_STRING_DIALECTS {
        for &to in ALL_STRING_DIALECTS {
            let translated = m.translate_event(from, to, &delta).unwrap();
            match &translated.kind {
                AgentEventKind::AssistantDelta { text } => {
                    assert_eq!(text, "", "empty delta should stay empty {from} -> {to}");
                }
                other => panic!("expected AssistantDelta, got {other:?}"),
            }
        }
    }
}

// =========================================================================
// 4. Capability Fidelity (5 tests)
// =========================================================================

#[test]
fn all_six_dialects_report_capability_support_levels() {
    // Each SDK dialect must report at least streaming + core tool capabilities.
    let core_caps = [Capability::Streaming, Capability::ToolRead];
    for (name, manifest) in all_sdk_manifests() {
        for cap in &core_caps {
            let level = manifest.get(cap);
            assert!(level.is_some(), "{name} must report {cap:?} capability");
            match level.unwrap() {
                SupportLevel::Native | SupportLevel::Emulated => {}
                other => panic!("{name} has unexpected {cap:?} level: {other:?}"),
            }
        }
        // All dialects must report ToolWrite and ToolBash, even if unsupported.
        for cap in &[Capability::ToolWrite, Capability::ToolBash] {
            assert!(
                manifest.contains_key(cap),
                "{name} must declare {cap:?} capability"
            );
        }
    }
}

#[test]
fn passthrough_mode_preserves_all_fields_same_dialect() {
    // Identity translation: same dialect → same dialect preserves everything.
    let wo = WorkOrderBuilder::new("Passthrough test task").build();
    for &d in Dialect::ALL {
        let result = translate(d, d, &wo).unwrap();
        let expected = serde_json::to_value(&wo).unwrap();
        assert_eq!(result, expected, "identity passthrough failed for {d:?}");
    }
}

#[test]
fn mapped_mode_correctly_degrades_unsupported_features() {
    // can_translate reports fidelity levels correctly.
    let m = matrix();
    // Identity = Lossless.
    for &d in Dialect::ALL {
        assert_eq!(
            m.can_translate(d, d),
            TranslationFidelity::Lossless,
            "identity should be lossless for {d:?}"
        );
    }
    // ABP → vendor = LossySupported.
    for &d in VENDOR_DIALECTS {
        assert_eq!(
            m.can_translate(Dialect::Abp, d),
            TranslationFidelity::LossySupported,
            "ABP -> {d:?} should be LossySupported"
        );
    }
    // Vendor → ABP = LossySupported.
    for &d in VENDOR_DIALECTS {
        assert_eq!(
            m.can_translate(d, Dialect::Abp),
            TranslationFidelity::LossySupported,
            "{d:?} -> ABP should be LossySupported"
        );
    }
}

#[test]
fn early_failure_when_critical_capability_unsupported() {
    // Vendor-to-vendor WorkOrder translation is unsupported in v0.1.
    let wo = WorkOrderBuilder::new("test").build();
    for &from in VENDOR_DIALECTS {
        for &to in VENDOR_DIALECTS {
            if from == to {
                continue;
            }
            let result = translate(from, to, &wo);
            assert!(
                result.is_err(),
                "vendor-to-vendor {from:?} -> {to:?} should fail"
            );
            let err_msg = result.unwrap_err().to_string();
            assert!(
                err_msg.contains("unsupported"),
                "error should mention unsupported for {from:?} -> {to:?}"
            );
        }
    }
}

#[test]
fn emulation_labeling_in_receipt_metadata() {
    // Capability manifests correctly label emulated capabilities.
    let claude_manifest = abp_claude_sdk::dialect::capability_manifest();
    // Claude reports Checkpointing as Emulated.
    assert!(
        matches!(
            claude_manifest.get(&Capability::Checkpointing),
            Some(SupportLevel::Emulated)
        ),
        "Claude should report Checkpointing as Emulated"
    );

    // Verify each manifest has a mix of support levels (not all the same).
    for (name, manifest) in all_sdk_manifests() {
        let levels: std::collections::HashSet<String> =
            manifest.values().map(|l| format!("{l:?}")).collect();
        assert!(
            levels.len() > 1,
            "{name} should have mixed support levels, got only: {levels:?}"
        );
    }
}

// =========================================================================
// 5. Edge Cases (5 tests)
// =========================================================================

#[test]
fn unicode_content_preserved_through_mapping() {
    let m = matrix();
    let unicode_samples = [
        "日本語テスト 🦀",
        "مرحبا بالعالم",
        "Ünïcödé: ñ ö ü",
        "emoji chain: 🔥🎉💻🌍🚀",
        "CJK+Hangul: 你好 こんにちは 안녕하세요",
        "math symbols: ∑∏∫∂√∞≠≤≥",
        "zalgo: h̸̡̪̯ẻ̶̡̺l̵̛̘l̴̪̽o̶̝̐",
    ];
    for content in &unicode_samples {
        let msgs = vec![Message {
            role: MessageRole::User,
            content: content.to_string(),
        }];
        for &from in Dialect::ALL {
            for &to in Dialect::ALL {
                let mapped = m.map_messages(from, to, &msgs).unwrap();
                assert_eq!(
                    mapped[0].content, *content,
                    "unicode lost for {from:?} -> {to:?}: {content}"
                );
            }
        }
    }
}

#[test]
fn very_long_messages_over_100kb() {
    let m = matrix();
    let long_text = "x".repeat(120_000); // 120KB
    let msgs = vec![Message {
        role: MessageRole::User,
        content: long_text.clone(),
    }];
    for &target in VENDOR_DIALECTS {
        let mapped = m.map_messages(Dialect::Abp, target, &msgs).unwrap();
        assert_eq!(
            mapped[0].content.len(),
            120_000,
            "long message truncated for {target:?}"
        );
        assert_eq!(mapped[0].content, long_text);
    }
}

#[test]
fn empty_messages_handled() {
    let m = matrix();
    let msgs = vec![Message {
        role: MessageRole::User,
        content: String::new(),
    }];
    for &from in Dialect::ALL {
        for &to in Dialect::ALL {
            let mapped = m.map_messages(from, to, &msgs).unwrap();
            assert_eq!(
                mapped[0].content, "",
                "empty content should stay empty {from:?} -> {to:?}"
            );
        }
    }

    // Empty message list.
    let empty: Vec<Message> = vec![];
    for &from in Dialect::ALL {
        for &to in Dialect::ALL {
            let mapped = m.map_messages(from, to, &empty).unwrap();
            assert!(
                mapped.is_empty(),
                "empty list should stay empty {from:?} -> {to:?}"
            );
        }
    }
}

#[test]
fn null_optional_fields_preserved() {
    let m = matrix();
    // ToolCall with all Nones.
    let call = ToolCall {
        tool_name: "read_file".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!(null),
    };
    for &from in ALL_STRING_DIALECTS {
        for &to in ALL_STRING_DIALECTS {
            let t = m.translate_tool_call(from, to, &call).unwrap();
            assert_eq!(
                t.tool_use_id, None,
                "tool_use_id should be None {from}->{to}"
            );
            assert_eq!(
                t.parent_tool_use_id, None,
                "parent_tool_use_id should be None {from}->{to}"
            );
            assert!(
                t.input.is_null(),
                "null input should stay null {from}->{to}"
            );
        }
    }

    // ToolResult with None id.
    let result = ToolResult {
        tool_name: "read_file".into(),
        tool_use_id: None,
        output: json!(null),
        is_error: false,
    };
    for &from in ALL_STRING_DIALECTS {
        for &to in ALL_STRING_DIALECTS {
            let t = m.translate_tool_result(from, to, &result).unwrap();
            assert_eq!(t.tool_use_id, None);
            assert!(t.output.is_null());
        }
    }
}

#[test]
fn special_characters_in_tool_names() {
    let m = matrix();
    let special_names = [
        "tool-with-dashes",
        "tool.with.dots",
        "tool_with_underscores",
        "tool/with/slashes",
        "tool with spaces",
        "tool@special#chars!",
        "工具名称",      // Chinese tool name
        "🔧wrench_tool", // emoji prefix
    ];
    for name in &special_names {
        let call = ToolCall {
            tool_name: name.to_string(),
            tool_use_id: Some("tc-special".into()),
            parent_tool_use_id: None,
            input: json!({"key": "value"}),
        };
        // Unmapped names pass through unchanged.
        for &from in ALL_STRING_DIALECTS {
            for &to in ALL_STRING_DIALECTS {
                let t = m.translate_tool_call(from, to, &call).unwrap();
                assert_eq!(
                    t.tool_name, *name,
                    "special tool name '{name}' should pass through {from} -> {to}"
                );
            }
        }
    }
}

// =========================================================================
// 6. Additional Cross-SDK Mapping Tests
// =========================================================================

#[test]
fn tool_def_identity_preserves_all_fields() {
    let m = matrix();
    let tools = vec![
        make_tool_def(
            "read_file",
            "Read a file",
            json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        ),
        make_tool_def("bash", "Run command", json!({"type": "object"})),
    ];
    for &d in Dialect::ALL {
        let mapped = m.map_tool_definitions(d, d, &tools).unwrap();
        assert_eq!(mapped.len(), 2);
        assert_eq!(mapped[0].name, "read_file");
        assert_eq!(mapped[1].name, "bash");
        assert_eq!(mapped[0].description, "Read a file");
    }
}

#[test]
fn event_mapping_tool_call_names_roundtrip_seven_dialects() {
    // Roundtrip through ABP for all seven string dialects.
    let m = matrix();
    for &dialect in ALL_STRING_DIALECTS {
        if dialect == "abp" {
            continue;
        }
        let forward = m.event_mapping(dialect, "abp");
        let backward = m.event_mapping("abp", dialect);
        assert!(
            forward.is_some(),
            "event mapping {dialect} -> abp should exist"
        );
        assert!(
            backward.is_some(),
            "event mapping abp -> {dialect} should exist"
        );
    }
}

#[test]
fn model_mapping_native_models_passthrough() {
    let m = matrix();
    let native_pairs: &[(Dialect, &str)] = &[
        (Dialect::OpenAi, "gpt-4o"),
        (Dialect::Claude, "claude-sonnet-4-20250514"),
        (Dialect::Gemini, "gemini-2.5-flash"),
        (Dialect::Codex, "codex-mini-latest"),
        (Dialect::Kimi, "moonshot-v1-8k"),
    ];
    for &(dialect, model) in native_pairs {
        let result = m.map_model_name(Dialect::Abp, dialect, model).unwrap();
        assert_eq!(
            result, model,
            "native model {model} should pass through for {dialect:?}"
        );
    }
}

#[test]
fn model_mapping_cross_dialect_equivalence() {
    let m = matrix();
    // gpt-4o is equivalent to claude-sonnet-4-20250514 for Claude target.
    let result = m
        .map_model_name(Dialect::OpenAi, Dialect::Claude, "gpt-4o")
        .unwrap();
    assert_eq!(result, "claude-sonnet-4-20250514");

    // gpt-4o -> gemini-2.5-flash for Gemini target.
    let result = m
        .map_model_name(Dialect::OpenAi, Dialect::Gemini, "gpt-4o")
        .unwrap();
    assert_eq!(result, "gemini-2.5-flash");
}

#[test]
fn codex_kimi_mock_tool_translations_registered() {
    let m = matrix();
    // codex, kimi, mock should all have translation paths.
    let extended = &["codex", "kimi", "mock"];
    for &dialect in extended {
        assert!(
            m.has_translation("abp", dialect),
            "abp -> {dialect} should exist"
        );
        assert!(
            m.has_translation(dialect, "abp"),
            "{dialect} -> abp should exist"
        );
    }
}

#[test]
fn translation_fidelity_mock_is_lossy_supported() {
    let m = matrix();
    for &d in Dialect::ALL {
        if d == Dialect::Mock {
            continue;
        }
        assert_eq!(
            m.can_translate(d, Dialect::Mock),
            TranslationFidelity::LossySupported,
            "{d:?} -> Mock should be LossySupported"
        );
        assert_eq!(
            m.can_translate(Dialect::Mock, d),
            TranslationFidelity::LossySupported,
            "Mock -> {d:?} should be LossySupported"
        );
    }
}

#[test]
fn full_chain_through_codex_and_kimi() {
    // ABP -> codex -> openai -> kimi -> abp
    let m = matrix();
    let chain: &[&str] = &["abp", "codex", "openai", "kimi", "abp"];
    let call = ToolCall {
        tool_name: "read_file".into(),
        tool_use_id: Some("tc-chain".into()),
        parent_tool_use_id: None,
        input: json!({"path": "Cargo.toml"}),
    };
    let mut current = call.clone();
    for window in chain.windows(2) {
        current = m
            .translate_tool_call(window[0], window[1], &current)
            .unwrap_or_else(|e| panic!("{} -> {} failed: {e}", window[0], window[1]));
    }
    assert_eq!(
        current.tool_name, "read_file",
        "should roundtrip to ABP name"
    );
    assert_eq!(current.input, call.input);
}

#[test]
fn event_warning_and_error_passthrough_all_dialects() {
    let m = matrix();
    let events = vec![
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Warning {
                message: "Rate limit approaching".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error {
                message: "Context window exceeded".into(),
            },
            ext: None,
        },
    ];
    for event in &events {
        for &from in ALL_STRING_DIALECTS {
            for &to in ALL_STRING_DIALECTS {
                let translated = m.translate_event(from, to, event).unwrap();
                match (&event.kind, &translated.kind) {
                    (
                        AgentEventKind::Warning { message: a },
                        AgentEventKind::Warning { message: b },
                    ) => assert_eq!(a, b),
                    (
                        AgentEventKind::Error { message: a },
                        AgentEventKind::Error { message: b },
                    ) => assert_eq!(a, b),
                    _ => panic!("kind mismatch {from} -> {to}"),
                }
            }
        }
    }
}

#[test]
fn batch_tool_definitions_mapped_correctly() {
    let m = matrix();
    let tools = vec![
        make_tool_def("read_file", "Read", json!({"type": "object"})),
        make_tool_def("write_file", "Write", json!({"type": "object"})),
        make_tool_def("bash", "Shell", json!({"type": "object"})),
        make_tool_def("edit_file", "Edit", json!({"type": "object"})),
        make_tool_def("glob", "Glob", json!({"type": "object"})),
    ];
    let mapped = m
        .map_tool_definitions(Dialect::Abp, Dialect::OpenAi, &tools)
        .unwrap();
    let names: Vec<&str> = mapped.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(
        names,
        &[
            "file_read",
            "file_write",
            "shell",
            "apply_diff",
            "file_search"
        ]
    );
}
