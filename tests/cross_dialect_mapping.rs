// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive cross-dialect mapping tests for the projection matrix.
//!
//! Covers the full dialect matrix, roundtrip integrity, edge cases,
//! and capability preservation for all supported vendor dialects.

use abp_core::{AgentEvent, AgentEventKind, Capability, SupportLevel, WorkOrderBuilder};
use abp_integrations::projection::{
    Dialect, ProjectionMatrix, ToolCall, ToolResult, supported_translations, translate,
};
use chrono::Utc;
use serde_json::json;
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn sample_wo() -> abp_core::WorkOrder {
    WorkOrderBuilder::new("Refactor the auth module").build()
}

fn make_tool_call(name: &str) -> ToolCall {
    ToolCall {
        tool_name: name.to_string(),
        tool_use_id: Some("tc-100".into()),
        parent_tool_use_id: None,
        input: json!({"path": "src/main.rs", "content": "fn main() {}"}),
    }
}

fn make_tool_result(name: &str) -> ToolResult {
    ToolResult {
        tool_name: name.to_string(),
        tool_use_id: Some("tc-100".into()),
        output: json!({"content": "fn main() {}", "ok": true}),
        is_error: false,
    }
}

fn make_tool_call_event(tool_name: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: tool_name.into(),
            tool_use_id: Some("tc-evt".into()),
            parent_tool_use_id: None,
            input: json!({"path": "lib.rs"}),
        },
        ext: None,
    }
}

fn make_tool_result_event(tool_name: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolResult {
            tool_name: tool_name.into(),
            tool_use_id: Some("tc-evt".into()),
            output: json!(["a.rs", "b.rs"]),
            is_error: false,
        },
        ext: None,
    }
}

/// The four string-based dialects used for tool/event translation.
const STRING_DIALECTS: &[&str] = &["abp", "openai", "anthropic", "gemini"];

/// Canonical ABP tool names used across all translations.
const ABP_TOOLS: &[&str] = &["read_file", "write_file", "bash", "edit_file", "glob"];

/// OpenAI tool names corresponding to ABP tools.
const OPENAI_TOOLS: &[&str] = &[
    "file_read",
    "file_write",
    "shell",
    "apply_diff",
    "file_search",
];

/// Anthropic tool names corresponding to ABP tools.
const ANTHROPIC_TOOLS: &[&str] = &["Read", "Write", "Bash", "Edit", "Glob"];

/// Gemini tool names corresponding to ABP tools.
const GEMINI_TOOLS: &[&str] = &[
    "readFile",
    "writeFile",
    "executeCommand",
    "editFile",
    "searchFiles",
];

/// Map a string dialect to its tool names.
fn tools_for_dialect(d: &str) -> &'static [&'static str] {
    match d {
        "abp" => ABP_TOOLS,
        "openai" => OPENAI_TOOLS,
        "anthropic" => ANTHROPIC_TOOLS,
        "gemini" => GEMINI_TOOLS,
        _ => &[],
    }
}

// =========================================================================
// 1. Full Dialect Matrix
// =========================================================================

// ---- WorkOrder translation: ABP → each vendor ----

#[test]
fn workorder_abp_to_all_vendor_dialects() {
    let wo = sample_wo();
    let vendor_dialects = [
        Dialect::Claude,
        Dialect::Codex,
        Dialect::Gemini,
        Dialect::Kimi,
        Dialect::OpenAi,
    ];
    for &target in &vendor_dialects {
        let result = translate(Dialect::Abp, target, &wo);
        assert!(
            result.is_ok(),
            "ABP -> {target:?} translation should succeed"
        );
        let val = result.unwrap();
        let obj = val.as_object().expect("result should be JSON object");
        assert!(
            obj.contains_key("model"),
            "{target:?} request must have model"
        );
    }
}

#[test]
fn workorder_identity_for_all_dialects() {
    let wo = sample_wo();
    let expected = serde_json::to_value(&wo).unwrap();
    for &d in Dialect::ALL {
        let result = translate(d, d, &wo).unwrap();
        assert_eq!(result, expected, "identity failed for {d:?}");
    }
}

#[test]
fn workorder_vendor_to_vendor_unsupported() {
    let wo = sample_wo();
    let vendors = [
        Dialect::Claude,
        Dialect::Codex,
        Dialect::Gemini,
        Dialect::Kimi,
        Dialect::OpenAi,
    ];
    for &from in &vendors {
        for &to in &vendors {
            if from == to {
                continue;
            }
            let result = translate(from, to, &wo);
            assert!(
                result.is_err(),
                "{from:?} -> {to:?} should be unsupported in v0.1"
            );
            assert!(
                result.unwrap_err().to_string().contains("unsupported"),
                "error should mention 'unsupported'"
            );
        }
    }
}

// ---- Tool call translation: all string dialect pairs ----

#[test]
fn tool_call_translation_all_registered_pairs() {
    let matrix = ProjectionMatrix::new();
    for &from in STRING_DIALECTS {
        for &to in STRING_DIALECTS {
            let tools = tools_for_dialect(from);
            for &tool in tools {
                let call = make_tool_call(tool);
                let result = matrix.translate_tool_call(from, to, &call);
                assert!(
                    result.is_ok(),
                    "translate_tool_call({from} -> {to}, {tool}) failed: {:?}",
                    result.err()
                );
                let translated = result.unwrap();
                // Input and IDs must be preserved.
                assert_eq!(translated.input, call.input);
                assert_eq!(translated.tool_use_id, call.tool_use_id);
                if from == to {
                    assert_eq!(
                        translated.tool_name, call.tool_name,
                        "identity should preserve name"
                    );
                }
            }
        }
    }
}

#[test]
fn tool_call_name_mapping_correctness() {
    let matrix = ProjectionMatrix::new();

    // Verify each ABP tool maps to the expected name per dialect.
    let expected: &[(&str, &[(&str, &str)])] = &[
        (
            "openai",
            &[
                ("read_file", "file_read"),
                ("write_file", "file_write"),
                ("bash", "shell"),
                ("edit_file", "apply_diff"),
                ("glob", "file_search"),
            ],
        ),
        (
            "anthropic",
            &[
                ("read_file", "Read"),
                ("write_file", "Write"),
                ("bash", "Bash"),
                ("edit_file", "Edit"),
                ("glob", "Glob"),
            ],
        ),
        (
            "gemini",
            &[
                ("read_file", "readFile"),
                ("write_file", "writeFile"),
                ("bash", "executeCommand"),
                ("edit_file", "editFile"),
                ("glob", "searchFiles"),
            ],
        ),
    ];

    for &(target, mappings) in expected {
        for &(abp_name, target_name) in mappings {
            let call = make_tool_call(abp_name);
            let translated = matrix.translate_tool_call("abp", target, &call).unwrap();
            assert_eq!(
                translated.tool_name, target_name,
                "abp/{abp_name} -> {target} expected {target_name}"
            );
        }
    }
}

// ---- Tool result translation: all string dialect pairs ----

#[test]
fn tool_result_translation_all_registered_pairs() {
    let matrix = ProjectionMatrix::new();
    for &from in STRING_DIALECTS {
        for &to in STRING_DIALECTS {
            let tools = tools_for_dialect(from);
            for &tool in tools {
                let result = make_tool_result(tool);
                let translated = matrix
                    .translate_tool_result(from, to, &result)
                    .unwrap_or_else(|e| {
                        panic!("translate_tool_result({from} -> {to}, {tool}) failed: {e}")
                    });
                assert_eq!(translated.output, result.output);
                assert_eq!(translated.is_error, result.is_error);
                assert_eq!(translated.tool_use_id, result.tool_use_id);
            }
        }
    }
}

// ---- Event mapping: all string dialect pairs ----

#[test]
fn event_translation_tool_call_all_pairs() {
    let matrix = ProjectionMatrix::new();
    for &from in STRING_DIALECTS {
        let tools = tools_for_dialect(from);
        for &to in STRING_DIALECTS {
            for &tool in tools {
                let event = make_tool_call_event(tool);
                let result = matrix.translate_event(from, to, &event);
                assert!(
                    result.is_ok(),
                    "translate_event({from} -> {to}, ToolCall/{tool}) failed"
                );
                let translated = result.unwrap();
                assert_eq!(translated.ts, event.ts, "timestamp must be preserved");
                match &translated.kind {
                    AgentEventKind::ToolCall { input, .. } => {
                        assert_eq!(input, &json!({"path": "lib.rs"}));
                    }
                    other => panic!("expected ToolCall, got {other:?}"),
                }
            }
        }
    }
}

#[test]
fn event_translation_tool_result_all_pairs() {
    let matrix = ProjectionMatrix::new();
    for &from in STRING_DIALECTS {
        let tools = tools_for_dialect(from);
        for &to in STRING_DIALECTS {
            for &tool in tools {
                let event = make_tool_result_event(tool);
                let translated = matrix
                    .translate_event(from, to, &event)
                    .unwrap_or_else(|e| {
                        panic!("translate_event({from} -> {to}, ToolResult/{tool}) failed: {e}")
                    });
                match &translated.kind {
                    AgentEventKind::ToolResult { is_error, .. } => {
                        assert!(!*is_error);
                    }
                    other => panic!("expected ToolResult, got {other:?}"),
                }
            }
        }
    }
}

#[test]
fn event_translation_non_tool_passthrough_all_pairs() {
    let matrix = ProjectionMatrix::new();
    let events = vec![
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Hello!".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "chunk".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "starting".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        },
    ];
    for &from in STRING_DIALECTS {
        for &to in STRING_DIALECTS {
            for event in &events {
                let translated = matrix.translate_event(from, to, event).unwrap_or_else(|e| {
                    panic!("non-tool event passthrough ({from} -> {to}) failed: {e}")
                });
                assert_eq!(translated.ts, event.ts);
                // Kind should be unchanged since non-tool events pass through.
                match (&event.kind, &translated.kind) {
                    (
                        AgentEventKind::AssistantMessage { text: a },
                        AgentEventKind::AssistantMessage { text: b },
                    ) => assert_eq!(a, b),
                    (
                        AgentEventKind::AssistantDelta { text: a },
                        AgentEventKind::AssistantDelta { text: b },
                    ) => assert_eq!(a, b),
                    (
                        AgentEventKind::RunStarted { message: a },
                        AgentEventKind::RunStarted { message: b },
                    ) => assert_eq!(a, b),
                    (
                        AgentEventKind::RunCompleted { message: a },
                        AgentEventKind::RunCompleted { message: b },
                    ) => assert_eq!(a, b),
                    _ => panic!("kind mismatch for {from} -> {to}"),
                }
            }
        }
    }
}

// ---- Event mapping table verification ----

#[test]
fn event_mapping_tables_exist_for_all_cross_pairs() {
    let matrix = ProjectionMatrix::new();
    for &from in STRING_DIALECTS {
        for &to in STRING_DIALECTS {
            if from == to {
                continue;
            }
            let mapping = matrix.event_mapping(from, to);
            assert!(
                mapping.is_some(),
                "event mapping should exist for {from} -> {to}"
            );
            assert!(
                !mapping.unwrap().kind_map.is_empty(),
                "event mapping {from} -> {to} should not be empty"
            );
        }
    }
}

#[test]
fn tool_translation_tables_exist_for_all_cross_pairs() {
    let matrix = ProjectionMatrix::new();
    for &from in STRING_DIALECTS {
        for &to in STRING_DIALECTS {
            if from == to {
                continue;
            }
            let tt = matrix.tool_translation(from, to);
            assert!(
                tt.is_some(),
                "tool translation should exist for {from} -> {to}"
            );
            assert!(
                !tt.unwrap().name_map.is_empty(),
                "tool translation {from} -> {to} should not be empty"
            );
        }
    }
}

// =========================================================================
// 2. Roundtrip Integrity
// =========================================================================

#[test]
fn roundtrip_tool_call_all_dialect_pairs() {
    let matrix = ProjectionMatrix::new();
    for &from in STRING_DIALECTS {
        for &to in STRING_DIALECTS {
            if from == to {
                continue;
            }
            let tools = tools_for_dialect(from);
            for &tool in tools {
                let original = make_tool_call(tool);
                let forward = matrix.translate_tool_call(from, to, &original).unwrap();
                let back = matrix.translate_tool_call(to, from, &forward).unwrap();
                assert_eq!(
                    back.tool_name, original.tool_name,
                    "roundtrip {from} -> {to} -> {from} lost tool name for {tool}"
                );
                assert_eq!(back.input, original.input, "input must survive roundtrip");
                assert_eq!(
                    back.tool_use_id, original.tool_use_id,
                    "tool_use_id must survive roundtrip"
                );
            }
        }
    }
}

#[test]
fn roundtrip_tool_result_all_dialect_pairs() {
    let matrix = ProjectionMatrix::new();
    for &from in STRING_DIALECTS {
        for &to in STRING_DIALECTS {
            if from == to {
                continue;
            }
            let tools = tools_for_dialect(from);
            for &tool in tools {
                let original = make_tool_result(tool);
                let forward = matrix.translate_tool_result(from, to, &original).unwrap();
                let back = matrix.translate_tool_result(to, from, &forward).unwrap();
                assert_eq!(
                    back.tool_name, original.tool_name,
                    "roundtrip {from} -> {to} -> {from} lost tool name"
                );
                assert_eq!(back.output, original.output);
                assert_eq!(back.is_error, original.is_error);
            }
        }
    }
}

#[test]
fn roundtrip_simple_text_messages() {
    let matrix = ProjectionMatrix::new();
    let messages = vec![
        "Hello, world!",
        "",
        "Multi\nline\ncontent",
        "Unicode: 日本語テスト 🦀",
    ];
    for &from in STRING_DIALECTS {
        for &to in STRING_DIALECTS {
            for msg in &messages {
                let event = AgentEvent {
                    ts: Utc::now(),
                    kind: AgentEventKind::AssistantMessage {
                        text: msg.to_string(),
                    },
                    ext: None,
                };
                let forward = matrix.translate_event(from, to, &event).unwrap();
                let back = matrix.translate_event(to, from, &forward).unwrap();
                match (&event.kind, &back.kind) {
                    (
                        AgentEventKind::AssistantMessage { text: a },
                        AgentEventKind::AssistantMessage { text: b },
                    ) => assert_eq!(a, b, "text roundtrip failed for {from} -> {to}"),
                    _ => panic!("kind mismatch"),
                }
            }
        }
    }
}

#[test]
fn roundtrip_tool_calls_with_complex_arguments() {
    let matrix = ProjectionMatrix::new();
    let complex_input = json!({
        "path": "src/deep/nested/file.rs",
        "content": "fn complex() { let x = vec![1, 2, 3]; }",
        "options": {
            "create_dirs": true,
            "permissions": "0o755",
            "metadata": {
                "author": "test",
                "tags": ["rust", "test"]
            }
        },
        "empty_obj": {},
        "empty_arr": [],
        "null_val": null,
        "number": 42,
        "float": 2.78,
        "bool": true
    });
    for &from in STRING_DIALECTS {
        for &to in STRING_DIALECTS {
            if from == to {
                continue;
            }
            let call = ToolCall {
                tool_name: tools_for_dialect(from)[0].to_string(),
                tool_use_id: Some("tc-complex".into()),
                parent_tool_use_id: Some("tc-parent".into()),
                input: complex_input.clone(),
            };
            let forward = matrix.translate_tool_call(from, to, &call).unwrap();
            let back = matrix.translate_tool_call(to, from, &forward).unwrap();
            assert_eq!(back.input, call.input, "complex args roundtrip failed");
            assert_eq!(back.parent_tool_use_id, call.parent_tool_use_id);
        }
    }
}

#[test]
fn roundtrip_tool_results_with_content() {
    let matrix = ProjectionMatrix::new();
    let outputs = vec![
        json!("simple string"),
        json!({"structured": true, "data": [1, 2, 3]}),
        json!(null),
        json!([]),
        json!(42),
    ];
    for &from in STRING_DIALECTS {
        for &to in STRING_DIALECTS {
            if from == to {
                continue;
            }
            for output in &outputs {
                let result = ToolResult {
                    tool_name: tools_for_dialect(from)[0].to_string(),
                    tool_use_id: Some("tc-rt".into()),
                    output: output.clone(),
                    is_error: false,
                };
                let forward = matrix.translate_tool_result(from, to, &result).unwrap();
                let back = matrix.translate_tool_result(to, from, &forward).unwrap();
                assert_eq!(back.output, result.output, "output roundtrip failed");
            }
        }
    }
}

#[test]
fn roundtrip_multi_step_conversation() {
    let matrix = ProjectionMatrix::new();
    // Simulate: ABP → OpenAI → Anthropic → Gemini → ABP
    let chain: &[&str] = &["abp", "openai", "anthropic", "gemini", "abp"];

    for &abp_tool in ABP_TOOLS {
        let mut call = make_tool_call(abp_tool);
        for window in chain.windows(2) {
            let (from, to) = (window[0], window[1]);
            call = matrix
                .translate_tool_call(from, to, &call)
                .unwrap_or_else(|e| panic!("{from} -> {to} failed: {e}"));
        }
        // After full chain, should be back to ABP name.
        assert_eq!(
            call.tool_name, abp_tool,
            "full chain roundtrip failed for {abp_tool}"
        );
    }
}

#[test]
fn roundtrip_event_with_ext_preserved() {
    let matrix = ProjectionMatrix::new();
    let mut ext = BTreeMap::new();
    ext.insert("custom_key".to_string(), json!({"nested": true}));
    ext.insert("version".to_string(), json!("1.0"));
    for &from in STRING_DIALECTS {
        for &to in STRING_DIALECTS {
            let event = AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::ToolCall {
                    tool_name: tools_for_dialect(from)[0].into(),
                    tool_use_id: Some("tc-ext-rt".into()),
                    parent_tool_use_id: None,
                    input: json!({}),
                },
                ext: Some(ext.clone()),
            };
            let forward = matrix.translate_event(from, to, &event).unwrap();
            assert_eq!(
                forward.ext,
                Some(ext.clone()),
                "ext not preserved {from} -> {to}"
            );
            let back = matrix.translate_event(to, from, &forward).unwrap();
            assert_eq!(back.ext, Some(ext.clone()), "ext not preserved roundtrip");
        }
    }
}

// =========================================================================
// 3. Edge Cases
// =========================================================================

#[test]
fn edge_empty_tool_name_all_pairs() {
    let matrix = ProjectionMatrix::new();
    let call = ToolCall {
        tool_name: String::new(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!({}),
    };
    for &from in STRING_DIALECTS {
        for &to in STRING_DIALECTS {
            let translated = matrix.translate_tool_call(from, to, &call).unwrap();
            // Empty names should pass through since they have no mapping.
            assert_eq!(
                translated.tool_name, "",
                "empty tool name should pass through for {from} -> {to}"
            );
        }
    }
}

#[test]
fn edge_empty_tool_arguments() {
    let matrix = ProjectionMatrix::new();
    let call = ToolCall {
        tool_name: "read_file".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!({}),
    };
    for &to in STRING_DIALECTS {
        let translated = matrix.translate_tool_call("abp", to, &call).unwrap();
        assert_eq!(
            translated.input,
            json!({}),
            "empty args should be preserved"
        );
    }
}

#[test]
fn edge_very_long_content_string() {
    let matrix = ProjectionMatrix::new();
    let long_content = "x".repeat(100_000);
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: long_content.clone(),
        },
        ext: None,
    };
    for &from in STRING_DIALECTS {
        for &to in STRING_DIALECTS {
            let translated = matrix.translate_event(from, to, &event).unwrap();
            match &translated.kind {
                AgentEventKind::AssistantMessage { text } => {
                    assert_eq!(text.len(), 100_000, "long content truncated");
                    assert_eq!(text, &long_content);
                }
                other => panic!("expected AssistantMessage, got {other:?}"),
            }
        }
    }
}

#[test]
fn edge_unicode_content() {
    let matrix = ProjectionMatrix::new();
    let unicode_strings = vec![
        "日本語テスト",
        "Ünïcödé with ñ and ö",
        "emoji: 🦀🔥🎉💻",
        "mixed: hello 世界 🌍",
        "CJK: 你好世界 こんにちは 안녕하세요",
        "RTL: مرحبا بالعالم",
        "math: ∑∏∫∂√∞",
        "box: ┌─┐│└┘",
        "zero-width: \u{200B}\u{200C}\u{200D}",
    ];
    for content in &unicode_strings {
        let call = ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({"content": content}),
        };
        for &from in STRING_DIALECTS {
            for &to in STRING_DIALECTS {
                let translated = matrix.translate_tool_call(from, to, &call).unwrap();
                assert_eq!(
                    translated.input["content"].as_str().unwrap(),
                    *content,
                    "unicode content lost for {from} -> {to}"
                );
            }
        }
    }
}

#[test]
fn edge_missing_optional_fields() {
    let matrix = ProjectionMatrix::new();
    // Tool call with all optional fields set to None.
    let call = ToolCall {
        tool_name: "read_file".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!(null),
    };
    for &from in STRING_DIALECTS {
        for &to in STRING_DIALECTS {
            let translated = matrix.translate_tool_call(from, to, &call).unwrap();
            assert_eq!(translated.tool_use_id, None);
            assert_eq!(translated.parent_tool_use_id, None);
        }
    }

    // Tool result with None ID.
    let result = ToolResult {
        tool_name: "read_file".into(),
        tool_use_id: None,
        output: json!(null),
        is_error: false,
    };
    for &from in STRING_DIALECTS {
        for &to in STRING_DIALECTS {
            let translated = matrix.translate_tool_result(from, to, &result).unwrap();
            assert_eq!(translated.tool_use_id, None);
        }
    }
}

#[test]
fn edge_unknown_model_names_in_workorder() {
    // Unknown model names should be passed through as-is.
    let models = vec![
        "totally-made-up-model-v99",
        "",
        "gpt-999-turbo-ultra",
        "claude-99.9-opus",
        "模型名称",
    ];
    for model in &models {
        let wo = WorkOrderBuilder::new("test task").model(*model).build();
        for &target in &[
            Dialect::Claude,
            Dialect::Codex,
            Dialect::Gemini,
            Dialect::Kimi,
            Dialect::OpenAi,
        ] {
            let val = translate(Dialect::Abp, target, &wo).unwrap();
            let actual_model = val.get("model").and_then(|m| m.as_str()).unwrap();
            assert_eq!(
                actual_model, *model,
                "model name should pass through for {target:?}"
            );
        }
    }
}

#[test]
fn edge_unmapped_tool_names_passthrough() {
    let matrix = ProjectionMatrix::new();
    let custom_tools = vec![
        "my_custom_tool",
        "unknown_tool_xyz",
        "🔧",
        "",
        "a_very_long_tool_name_that_nobody_would_ever_use_in_practice_but_should_still_work",
    ];
    for tool_name in &custom_tools {
        let call = ToolCall {
            tool_name: tool_name.to_string(),
            tool_use_id: Some("tc-custom".into()),
            parent_tool_use_id: None,
            input: json!({}),
        };
        for &from in STRING_DIALECTS {
            for &to in STRING_DIALECTS {
                let translated = matrix.translate_tool_call(from, to, &call).unwrap();
                // Unmapped names always pass through unchanged.
                assert_eq!(
                    translated.tool_name, *tool_name,
                    "unmapped tool '{tool_name}' should pass through {from} -> {to}"
                );
            }
        }
    }
}

#[test]
fn edge_unknown_dialect_errors() {
    let matrix = ProjectionMatrix::new();
    let call = make_tool_call("read_file");
    let result = make_tool_result("read_file");
    let event = make_tool_call_event("read_file");

    let bad_dialects = vec!["unknown", "Claude", "OPENAI", "Anthropic", "GEMINI", ""];
    for bad in &bad_dialects {
        // Unknown as target.
        assert!(
            matrix.translate_tool_call("abp", bad, &call).is_err(),
            "should reject unknown target dialect '{bad}'"
        );
        // Unknown as source.
        assert!(
            matrix.translate_tool_call(bad, "abp", &call).is_err(),
            "should reject unknown source dialect '{bad}'"
        );
        // Tool result.
        assert!(matrix.translate_tool_result("abp", bad, &result).is_err());
        assert!(matrix.translate_tool_result(bad, "abp", &result).is_err());
        // Event.
        assert!(matrix.translate_event("abp", bad, &event).is_err());
        assert!(matrix.translate_event(bad, "abp", &event).is_err());
    }
}

#[test]
fn edge_mixed_case_dialect_names_rejected() {
    let matrix = ProjectionMatrix::new();
    let call = make_tool_call("read_file");
    // String-based dialect names are case-sensitive; mixed case should fail.
    let mixed_cases = vec!["Abp", "ABP", "OpenAI", "OpenAi", "Anthropic", "Gemini"];
    for name in &mixed_cases {
        assert!(
            matrix.translate_tool_call(name, "abp", &call).is_err(),
            "mixed-case dialect '{name}' should be rejected"
        );
    }
}

#[test]
fn edge_tool_result_error_flag_all_pairs() {
    let matrix = ProjectionMatrix::new();
    let result = ToolResult {
        tool_name: "read_file".into(),
        tool_use_id: Some("tc-err".into()),
        output: json!({"error": "file not found"}),
        is_error: true,
    };
    for &from in STRING_DIALECTS {
        for &to in STRING_DIALECTS {
            let translated = matrix.translate_tool_result(from, to, &result).unwrap();
            assert!(
                translated.is_error,
                "is_error flag must be preserved for {from} -> {to}"
            );
        }
    }
}

#[test]
fn edge_event_with_file_changed_passthrough() {
    let matrix = ProjectionMatrix::new();
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::FileChanged {
            path: "src/main.rs".into(),
            summary: "Added main function".into(),
        },
        ext: None,
    };
    for &from in STRING_DIALECTS {
        for &to in STRING_DIALECTS {
            let translated = matrix.translate_event(from, to, &event).unwrap();
            match &translated.kind {
                AgentEventKind::FileChanged { path, summary } => {
                    assert_eq!(path, "src/main.rs");
                    assert_eq!(summary, "Added main function");
                }
                other => panic!("expected FileChanged, got {other:?}"),
            }
        }
    }
}

#[test]
fn edge_event_with_command_executed_passthrough() {
    let matrix = ProjectionMatrix::new();
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: None,
        },
        ext: None,
    };
    for &from in STRING_DIALECTS {
        for &to in STRING_DIALECTS {
            let translated = matrix.translate_event(from, to, &event).unwrap();
            match &translated.kind {
                AgentEventKind::CommandExecuted {
                    command,
                    exit_code,
                    output_preview,
                } => {
                    assert_eq!(command, "cargo test");
                    assert_eq!(*exit_code, Some(0));
                    assert_eq!(*output_preview, None);
                }
                other => panic!("expected CommandExecuted, got {other:?}"),
            }
        }
    }
}

#[test]
fn edge_null_json_input_preserved() {
    let matrix = ProjectionMatrix::new();
    let call = ToolCall {
        tool_name: "bash".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!(null),
    };
    for &from in STRING_DIALECTS {
        for &to in STRING_DIALECTS {
            let translated = matrix.translate_tool_call(from, to, &call).unwrap();
            assert!(
                translated.input.is_null(),
                "null input should be preserved for {from} -> {to}"
            );
        }
    }
}

// =========================================================================
// 4. Capability Preservation
// =========================================================================

#[test]
fn capability_manifests_non_empty_for_all_dialects() {
    let manifests: Vec<(&str, abp_core::CapabilityManifest)> = vec![
        ("claude", abp_claude_sdk::dialect::capability_manifest()),
        ("gemini", abp_gemini_sdk::dialect::capability_manifest()),
        ("codex", abp_codex_sdk::dialect::capability_manifest()),
        ("openai", abp_openai_sdk::dialect::capability_manifest()),
        ("kimi", abp_kimi_sdk::dialect::capability_manifest()),
    ];
    for (name, manifest) in &manifests {
        assert!(
            !manifest.is_empty(),
            "capability manifest for {name} should not be empty"
        );
    }
}

#[test]
fn streaming_support_reported_for_all_dialects() {
    let manifests: Vec<(&str, abp_core::CapabilityManifest)> = vec![
        ("claude", abp_claude_sdk::dialect::capability_manifest()),
        ("gemini", abp_gemini_sdk::dialect::capability_manifest()),
        ("codex", abp_codex_sdk::dialect::capability_manifest()),
        ("openai", abp_openai_sdk::dialect::capability_manifest()),
        ("kimi", abp_kimi_sdk::dialect::capability_manifest()),
    ];
    for (name, manifest) in &manifests {
        let streaming = manifest.get(&Capability::Streaming);
        assert!(
            streaming.is_some(),
            "{name} should report streaming capability"
        );
        // All known dialects support streaming natively.
        assert!(
            matches!(streaming.unwrap(), SupportLevel::Native),
            "{name} should have native streaming"
        );
    }
}

#[test]
fn mcp_support_reported_for_all_dialects() {
    let manifests: Vec<(&str, abp_core::CapabilityManifest)> = vec![
        ("claude", abp_claude_sdk::dialect::capability_manifest()),
        ("gemini", abp_gemini_sdk::dialect::capability_manifest()),
        ("codex", abp_codex_sdk::dialect::capability_manifest()),
        ("openai", abp_openai_sdk::dialect::capability_manifest()),
        ("kimi", abp_kimi_sdk::dialect::capability_manifest()),
    ];
    for (name, manifest) in &manifests {
        // Every dialect should declare MCP client support level.
        assert!(
            manifest.contains_key(&Capability::McpClient),
            "{name} should declare McpClient capability"
        );
        // Every dialect should declare MCP server support level.
        assert!(
            manifest.contains_key(&Capability::McpServer),
            "{name} should declare McpServer capability"
        );
    }
}

#[test]
fn claude_has_native_mcp_client() {
    let manifest = abp_claude_sdk::dialect::capability_manifest();
    assert!(
        matches!(
            manifest.get(&Capability::McpClient).unwrap(),
            SupportLevel::Native
        ),
        "Claude should have native MCP client support"
    );
}

#[test]
fn claude_has_unsupported_mcp_server() {
    let manifest = abp_claude_sdk::dialect::capability_manifest();
    assert!(
        matches!(
            manifest.get(&Capability::McpServer).unwrap(),
            SupportLevel::Unsupported
        ),
        "Claude should not support MCP server"
    );
}

#[test]
fn capability_manifests_have_consistent_tool_capabilities() {
    let manifests: Vec<(&str, abp_core::CapabilityManifest)> = vec![
        ("claude", abp_claude_sdk::dialect::capability_manifest()),
        ("gemini", abp_gemini_sdk::dialect::capability_manifest()),
        ("codex", abp_codex_sdk::dialect::capability_manifest()),
        ("openai", abp_openai_sdk::dialect::capability_manifest()),
        ("kimi", abp_kimi_sdk::dialect::capability_manifest()),
    ];
    let tool_caps = [Capability::ToolRead];
    for (name, manifest) in &manifests {
        for cap in &tool_caps {
            let level = manifest.get(cap);
            assert!(level.is_some(), "{name} should report {cap:?}");
            match level.unwrap() {
                SupportLevel::Native | SupportLevel::Emulated => {}
                other => panic!("{name} has unexpected {cap:?} support: {other:?}"),
            }
        }
    }
}

// =========================================================================
// Structural / Matrix Coverage
// =========================================================================

#[test]
fn supported_translations_count() {
    let pairs = supported_translations();
    // 7 identity + 6 ABP-to-vendor = 13 (with Mock dialect).
    assert_eq!(pairs.len(), 13, "expected 13 supported translation pairs");
}

#[test]
fn has_translation_covers_all_registered_string_pairs() {
    let matrix = ProjectionMatrix::new();
    for &from in STRING_DIALECTS {
        for &to in STRING_DIALECTS {
            assert!(
                matrix.has_translation(from, to),
                "has_translation should be true for {from} -> {to}"
            );
        }
    }
}

#[test]
fn supported_dialects_includes_all_four() {
    let matrix = ProjectionMatrix::new();
    let dialects = matrix.supported_dialects();
    for &expected in STRING_DIALECTS {
        assert!(
            dialects.contains(&expected.to_string()),
            "missing dialect: {expected}"
        );
    }
}

#[test]
fn dialect_enum_all_covers_six_variants() {
    assert_eq!(Dialect::ALL.len(), 7, "should have 7 dialect variants");
    let names: Vec<_> = Dialect::ALL.iter().map(|d| format!("{d:?}")).collect();
    for expected in &["Abp", "Claude", "Codex", "Gemini", "Kimi", "Mock", "OpenAi"] {
        assert!(
            names.iter().any(|n| n == expected),
            "missing Dialect::{expected}"
        );
    }
}

#[test]
fn dialect_serde_roundtrip() {
    for &d in Dialect::ALL {
        let json = serde_json::to_string(&d).unwrap();
        let back: Dialect = serde_json::from_str(&json).unwrap();
        assert_eq!(back, d, "serde roundtrip failed for {d:?}");
    }
}

#[test]
fn workorder_abp_to_claude_has_messages_and_max_tokens() {
    let wo = sample_wo();
    let val = translate(Dialect::Abp, Dialect::Claude, &wo).unwrap();
    let obj = val.as_object().unwrap();
    assert!(obj.contains_key("messages"));
    assert!(obj.contains_key("max_tokens"));
}

#[test]
fn workorder_abp_to_codex_has_input() {
    let wo = sample_wo();
    let val = translate(Dialect::Abp, Dialect::Codex, &wo).unwrap();
    let obj = val.as_object().unwrap();
    assert!(obj.contains_key("input"));
    assert!(obj.contains_key("max_output_tokens"));
}

#[test]
fn workorder_abp_to_gemini_has_contents_and_generation_config() {
    let wo = sample_wo();
    let val = translate(Dialect::Abp, Dialect::Gemini, &wo).unwrap();
    let obj = val.as_object().unwrap();
    assert!(obj.contains_key("contents"));
    assert!(obj.contains_key("generation_config"));
}

#[test]
fn workorder_abp_to_kimi_has_messages() {
    let wo = sample_wo();
    let val = translate(Dialect::Abp, Dialect::Kimi, &wo).unwrap();
    let obj = val.as_object().unwrap();
    assert!(obj.contains_key("messages"));
    assert!(obj.contains_key("max_tokens"));
}

#[test]
fn workorder_default_models_correct() {
    let wo = sample_wo();
    let expectations: &[(Dialect, &str)] = &[
        (Dialect::Claude, "claude-sonnet-4-20250514"),
        (Dialect::Codex, "codex-mini-latest"),
        (Dialect::Gemini, "gemini-2.5-flash"),
        (Dialect::Kimi, "moonshot-v1-8k"),
        (Dialect::OpenAi, "gpt-4o"),
    ];
    for &(dialect, expected_model) in expectations {
        let val = translate(Dialect::Abp, dialect, &wo).unwrap();
        let model = val.get("model").and_then(|m| m.as_str()).unwrap();
        assert_eq!(
            model, expected_model,
            "default model for {dialect:?} should be {expected_model}"
        );
    }
}

#[test]
fn workorder_task_content_present_in_all_translations() {
    let task = "Implement feature X with tests";
    let wo = WorkOrderBuilder::new(task).build();
    for &dialect in &[
        Dialect::Claude,
        Dialect::Codex,
        Dialect::Gemini,
        Dialect::Kimi,
        Dialect::OpenAi,
    ] {
        let val = translate(Dialect::Abp, dialect, &wo).unwrap();
        let json_str = serde_json::to_string(&val).unwrap();
        assert!(
            json_str.contains(task),
            "task content missing in {dialect:?} translation"
        );
    }
}

#[test]
fn workorder_with_snippets_includes_content() {
    let mut wo = WorkOrderBuilder::new("Fix bug").build();
    wo.context.snippets.push(abp_core::ContextSnippet {
        name: "error.log".into(),
        content: "PANIC at line 42".into(),
    });
    for &dialect in &[Dialect::Claude, Dialect::OpenAi, Dialect::Gemini] {
        let val = translate(Dialect::Abp, dialect, &wo).unwrap();
        let json_str = serde_json::to_string(&val).unwrap();
        assert!(
            json_str.contains("PANIC at line 42"),
            "snippet content missing in {dialect:?}"
        );
        assert!(
            json_str.contains("error.log"),
            "snippet name missing in {dialect:?}"
        );
    }
}

// =========================================================================
// Cross-vendor direct translation chains
// =========================================================================

#[test]
fn full_chain_all_tool_names_through_four_dialects() {
    let matrix = ProjectionMatrix::new();
    // For each ABP tool, translate through the full chain.
    let chain: &[&str] = &["abp", "openai", "anthropic", "gemini", "abp"];

    let expected_intermediate: &[(&str, &str, &str, &str)] = &[
        ("read_file", "file_read", "Read", "readFile"),
        ("write_file", "file_write", "Write", "writeFile"),
        ("bash", "shell", "Bash", "executeCommand"),
        ("edit_file", "apply_diff", "Edit", "editFile"),
        ("glob", "file_search", "Glob", "searchFiles"),
    ];

    for &(abp, openai, anthropic, gemini) in expected_intermediate {
        let call = make_tool_call(abp);

        let step1 = matrix
            .translate_tool_call(chain[0], chain[1], &call)
            .unwrap();
        assert_eq!(step1.tool_name, openai, "abp -> openai");

        let step2 = matrix
            .translate_tool_call(chain[1], chain[2], &step1)
            .unwrap();
        assert_eq!(step2.tool_name, anthropic, "openai -> anthropic");

        let step3 = matrix
            .translate_tool_call(chain[2], chain[3], &step2)
            .unwrap();
        assert_eq!(step3.tool_name, gemini, "anthropic -> gemini");

        let step4 = matrix
            .translate_tool_call(chain[3], chain[4], &step3)
            .unwrap();
        assert_eq!(step4.tool_name, abp, "gemini -> abp");

        assert_eq!(step4.input, call.input, "input must survive full chain");
    }
}

#[test]
fn reverse_chain_all_tool_names() {
    let matrix = ProjectionMatrix::new();
    // Reverse: ABP → Gemini → Anthropic → OpenAI → ABP
    let chain: &[&str] = &["abp", "gemini", "anthropic", "openai", "abp"];

    for &abp_tool in ABP_TOOLS {
        let mut call = make_tool_call(abp_tool);
        for window in chain.windows(2) {
            call = matrix
                .translate_tool_call(window[0], window[1], &call)
                .unwrap();
        }
        assert_eq!(call.tool_name, abp_tool, "reverse chain roundtrip failed");
    }
}

#[test]
fn event_mapping_roundtrip_all_pairs() {
    let matrix = ProjectionMatrix::new();
    for &from in STRING_DIALECTS {
        for &to in STRING_DIALECTS {
            if from == to {
                continue;
            }
            let forward = matrix.event_mapping(from, to);
            let backward = matrix.event_mapping(to, from);
            if let (Some(fwd), Some(bwd)) = (forward, backward) {
                // Every key in forward should map to a key in backward.
                for (k, v) in &fwd.kind_map {
                    let back = bwd.kind_map.get(v);
                    assert!(
                        back.is_some(),
                        "event mapping {from}->{to} maps '{k}' to '{v}', but {to}->{from} has no entry for '{v}'"
                    );
                    assert_eq!(
                        back.unwrap(),
                        k,
                        "event mapping roundtrip: {from}->'{k}'->{to}->'{v}'->{from} should give '{k}'"
                    );
                }
            }
        }
    }
}
