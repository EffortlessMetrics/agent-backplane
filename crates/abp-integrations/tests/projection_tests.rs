// SPDX-License-Identifier: MIT OR Apache-2.0
use abp_core::{AgentEvent, AgentEventKind, WorkOrderBuilder};
use abp_integrations::projection::{
    Dialect, ProjectionMatrix, ToolCall, ToolResult, supported_translations, translate,
};
use chrono::Utc;

fn sample_work_order() -> abp_core::WorkOrder {
    WorkOrderBuilder::new("Refactor the auth module").build()
}

#[test]
fn identity_translation_preserves_data() {
    let wo = sample_work_order();
    let matrix = ProjectionMatrix::new();

    for &dialect in Dialect::ALL {
        let result = matrix.translate(dialect, dialect, &wo).unwrap();
        // Identity always serialises the original work order.
        let expected = serde_json::to_value(&wo).unwrap();
        assert_eq!(result, expected, "identity failed for {dialect:?}");
    }
}

#[test]
fn abp_to_claude_produces_valid_json() {
    let wo = sample_work_order();
    let val = translate(Dialect::Abp, Dialect::Claude, &wo).unwrap();
    let obj = val.as_object().expect("should be a JSON object");
    assert!(obj.contains_key("model"), "Claude request must have model");
    assert!(
        obj.contains_key("messages"),
        "Claude request must have messages"
    );
}

#[test]
fn abp_to_codex_produces_valid_json() {
    let wo = sample_work_order();
    let val = translate(Dialect::Abp, Dialect::Codex, &wo).unwrap();
    let obj = val.as_object().expect("should be a JSON object");
    assert!(obj.contains_key("model"), "Codex request must have model");
    assert!(obj.contains_key("input"), "Codex request must have input");
}

#[test]
fn abp_to_gemini_produces_valid_json() {
    let wo = sample_work_order();
    let val = translate(Dialect::Abp, Dialect::Gemini, &wo).unwrap();
    let obj = val.as_object().expect("should be a JSON object");
    assert!(obj.contains_key("model"), "Gemini request must have model");
    assert!(
        obj.contains_key("contents"),
        "Gemini request must have contents"
    );
}

#[test]
fn abp_to_kimi_produces_valid_json() {
    let wo = sample_work_order();
    let val = translate(Dialect::Abp, Dialect::Kimi, &wo).unwrap();
    let obj = val.as_object().expect("should be a JSON object");
    assert!(obj.contains_key("model"), "Kimi request must have model");
    assert!(
        obj.contains_key("messages"),
        "Kimi request must have messages"
    );
}

#[test]
fn unsupported_translation_returns_error() {
    let wo = sample_work_order();
    // Vendor-to-vendor (non-identity, non-ABP source) is unsupported in v0.1.
    let result = translate(Dialect::Claude, Dialect::Codex, &wo);
    assert!(result.is_err(), "Claude->Codex should be unsupported");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("unsupported"),
        "error should mention 'unsupported': {msg}"
    );
}

#[test]
fn supported_translations_includes_all_identity_pairs() {
    let pairs = supported_translations();
    for &dialect in Dialect::ALL {
        assert!(
            pairs.contains(&(dialect, dialect)),
            "missing identity pair for {dialect:?}"
        );
    }
}

#[test]
fn supported_translations_includes_abp_to_vendor_pairs() {
    let pairs = supported_translations();
    for &dialect in Dialect::ALL {
        if dialect != Dialect::Abp {
            assert!(
                pairs.contains(&(Dialect::Abp, dialect)),
                "missing ABP->{dialect:?} pair"
            );
        }
    }
}

#[test]
fn projection_matrix_struct_matches_free_functions() {
    let wo = sample_work_order();
    let matrix = ProjectionMatrix::new();

    // Method should give the same results as the free function.
    let method_result = matrix
        .translate(Dialect::Abp, Dialect::Claude, &wo)
        .unwrap();
    let free_result = translate(Dialect::Abp, Dialect::Claude, &wo).unwrap();
    assert_eq!(method_result, free_result);

    assert_eq!(matrix.supported_translations(), supported_translations());
}

// ---------------------------------------------------------------------------
// Tool call translation tests
// ---------------------------------------------------------------------------

fn sample_tool_call(name: &str) -> ToolCall {
    ToolCall {
        tool_name: name.to_string(),
        tool_use_id: Some("tc-1".into()),
        parent_tool_use_id: None,
        input: serde_json::json!({"path": "src/main.rs"}),
    }
}

fn sample_tool_result(name: &str) -> ToolResult {
    ToolResult {
        tool_name: name.to_string(),
        tool_use_id: Some("tc-1".into()),
        output: serde_json::json!({"content": "fn main() {}"}),
        is_error: false,
    }
}

#[test]
fn translate_tool_call_abp_to_openai() {
    let matrix = ProjectionMatrix::new();
    let call = sample_tool_call("read_file");
    let translated = matrix.translate_tool_call("abp", "openai", &call).unwrap();
    assert_eq!(translated.tool_name, "file_read");
    assert_eq!(translated.tool_use_id, call.tool_use_id);
    assert_eq!(translated.input, call.input);
}

#[test]
fn translate_tool_call_abp_to_anthropic() {
    let matrix = ProjectionMatrix::new();
    let call = sample_tool_call("bash");
    let translated = matrix
        .translate_tool_call("abp", "anthropic", &call)
        .unwrap();
    assert_eq!(translated.tool_name, "Bash");
}

#[test]
fn translate_tool_call_abp_to_gemini() {
    let matrix = ProjectionMatrix::new();
    let call = sample_tool_call("edit_file");
    let translated = matrix.translate_tool_call("abp", "gemini", &call).unwrap();
    assert_eq!(translated.tool_name, "editFile");
}

#[test]
fn translate_tool_result_abp_to_openai() {
    let matrix = ProjectionMatrix::new();
    let result = sample_tool_result("write_file");
    let translated = matrix
        .translate_tool_result("abp", "openai", &result)
        .unwrap();
    assert_eq!(translated.tool_name, "file_write");
    assert_eq!(translated.output, result.output);
    assert!(!translated.is_error);
}

#[test]
fn translate_event_with_tool_call() {
    let matrix = ProjectionMatrix::new();
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tc-1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"path": "lib.rs"}),
        },
        ext: None,
    };
    let translated = matrix.translate_event("abp", "anthropic", &event).unwrap();
    match &translated.kind {
        AgentEventKind::ToolCall { tool_name, .. } => {
            assert_eq!(tool_name, "Read");
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn translate_event_with_tool_result() {
    let matrix = ProjectionMatrix::new();
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolResult {
            tool_name: "glob".into(),
            tool_use_id: Some("tc-2".into()),
            output: serde_json::json!(["a.rs", "b.rs"]),
            is_error: false,
        },
        ext: None,
    };
    let translated = matrix.translate_event("abp", "gemini", &event).unwrap();
    match &translated.kind {
        AgentEventKind::ToolResult {
            tool_name,
            is_error,
            ..
        } => {
            assert_eq!(tool_name, "searchFiles");
            assert!(!*is_error);
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn translate_event_non_tool_passthrough() {
    let matrix = ProjectionMatrix::new();
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "Hello!".into(),
        },
        ext: None,
    };
    let translated = matrix.translate_event("abp", "openai", &event).unwrap();
    match &translated.kind {
        AgentEventKind::AssistantMessage { text } => {
            assert_eq!(text, "Hello!");
        }
        other => panic!("expected AssistantMessage, got {other:?}"),
    }
}

#[test]
fn unknown_dialect_returns_error() {
    let matrix = ProjectionMatrix::new();
    let call = sample_tool_call("read_file");
    let result = matrix.translate_tool_call("abp", "unknown_vendor", &call);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("unknown dialect"));
}

#[test]
fn unknown_source_dialect_returns_error() {
    let matrix = ProjectionMatrix::new();
    let call = sample_tool_call("read_file");
    let result = matrix.translate_tool_call("nonexistent", "abp", &call);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("unknown dialect"));
}

#[test]
fn identity_translation_tool_call() {
    let matrix = ProjectionMatrix::new();
    let call = sample_tool_call("read_file");
    let translated = matrix.translate_tool_call("abp", "abp", &call).unwrap();
    assert_eq!(translated, call);
}

#[test]
fn identity_translation_event() {
    let matrix = ProjectionMatrix::new();
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: serde_json::json!({}),
        },
        ext: None,
    };
    let translated = matrix.translate_event("openai", "openai", &event).unwrap();
    assert_eq!(translated.ts, event.ts);
    match (&translated.kind, &event.kind) {
        (
            AgentEventKind::ToolCall {
                tool_name: a_name, ..
            },
            AgentEventKind::ToolCall {
                tool_name: b_name, ..
            },
        ) => assert_eq!(a_name, b_name),
        _ => panic!("kind mismatch"),
    }
}

#[test]
fn round_trip_tool_call_abp_openai() {
    let matrix = ProjectionMatrix::new();
    let original = sample_tool_call("bash");
    let to_openai = matrix
        .translate_tool_call("abp", "openai", &original)
        .unwrap();
    assert_eq!(to_openai.tool_name, "shell");
    let back = matrix
        .translate_tool_call("openai", "abp", &to_openai)
        .unwrap();
    assert_eq!(back.tool_name, "bash");
    assert_eq!(back.input, original.input);
}

#[test]
fn round_trip_tool_call_abp_anthropic() {
    let matrix = ProjectionMatrix::new();
    let original = sample_tool_call("write_file");
    let to_anthropic = matrix
        .translate_tool_call("abp", "anthropic", &original)
        .unwrap();
    assert_eq!(to_anthropic.tool_name, "Write");
    let back = matrix
        .translate_tool_call("anthropic", "abp", &to_anthropic)
        .unwrap();
    assert_eq!(back.tool_name, "write_file");
}

#[test]
fn supported_dialects_list() {
    let matrix = ProjectionMatrix::new();
    let dialects = matrix.supported_dialects();
    assert!(dialects.contains(&"abp".to_string()));
    assert!(dialects.contains(&"openai".to_string()));
    assert!(dialects.contains(&"anthropic".to_string()));
    assert!(dialects.contains(&"gemini".to_string()));
    assert_eq!(dialects.len(), 4);
}

// ---------------------------------------------------------------------------
// OpenAI projection tests
// ---------------------------------------------------------------------------

#[test]
fn abp_to_openai_produces_valid_json() {
    let wo = sample_work_order();
    let val = translate(Dialect::Abp, Dialect::OpenAi, &wo).unwrap();
    let obj = val.as_object().expect("should be a JSON object");
    assert!(obj.contains_key("model"), "OpenAI request must have model");
    assert!(
        obj.contains_key("messages"),
        "OpenAI request must have messages"
    );
    assert!(
        obj.contains_key("max_tokens"),
        "OpenAI request must have max_tokens"
    );
}

#[test]
fn abp_to_openai_default_model() {
    let wo = sample_work_order();
    let val = translate(Dialect::Abp, Dialect::OpenAi, &wo).unwrap();
    let model = val.get("model").and_then(|m| m.as_str()).unwrap();
    assert_eq!(model, "gpt-4o");
}

#[test]
fn abp_to_openai_custom_model() {
    let wo = WorkOrderBuilder::new("task").model("gpt-4-turbo").build();
    let val = translate(Dialect::Abp, Dialect::OpenAi, &wo).unwrap();
    let model = val.get("model").and_then(|m| m.as_str()).unwrap();
    assert_eq!(model, "gpt-4-turbo");
}

#[test]
fn translate_tool_call_claude_to_openai() {
    let matrix = ProjectionMatrix::new();
    let call = sample_tool_call("Read");
    let translated = matrix
        .translate_tool_call("anthropic", "openai", &call)
        .unwrap();
    assert_eq!(translated.tool_name, "file_read");
}

#[test]
fn translate_tool_call_openai_to_gemini() {
    let matrix = ProjectionMatrix::new();
    let call = sample_tool_call("file_write");
    let translated = matrix
        .translate_tool_call("openai", "gemini", &call)
        .unwrap();
    assert_eq!(translated.tool_name, "writeFile");
}

#[test]
fn translate_tool_call_gemini_to_openai() {
    let matrix = ProjectionMatrix::new();
    let call = sample_tool_call("executeCommand");
    let translated = matrix
        .translate_tool_call("gemini", "openai", &call)
        .unwrap();
    assert_eq!(translated.tool_name, "shell");
}

#[test]
fn round_trip_tool_call_claude_openai() {
    let matrix = ProjectionMatrix::new();
    let original = sample_tool_call("Read");
    let to_openai = matrix
        .translate_tool_call("anthropic", "openai", &original)
        .unwrap();
    assert_eq!(to_openai.tool_name, "file_read");
    let back = matrix
        .translate_tool_call("openai", "anthropic", &to_openai)
        .unwrap();
    assert_eq!(back.tool_name, "Read");
    assert_eq!(back.input, original.input);
}

#[test]
fn round_trip_tool_call_openai_gemini() {
    let matrix = ProjectionMatrix::new();
    let original = sample_tool_call("file_search");
    let to_gemini = matrix
        .translate_tool_call("openai", "gemini", &original)
        .unwrap();
    assert_eq!(to_gemini.tool_name, "searchFiles");
    let back = matrix
        .translate_tool_call("gemini", "openai", &to_gemini)
        .unwrap();
    assert_eq!(back.tool_name, "file_search");
}

#[test]
fn round_trip_tool_call_openai_abp() {
    let matrix = ProjectionMatrix::new();
    let original = sample_tool_call("apply_diff");
    let to_abp = matrix
        .translate_tool_call("openai", "abp", &original)
        .unwrap();
    assert_eq!(to_abp.tool_name, "edit_file");
    let back = matrix
        .translate_tool_call("abp", "openai", &to_abp)
        .unwrap();
    assert_eq!(back.tool_name, "apply_diff");
}

#[test]
fn translate_tool_result_openai_to_anthropic() {
    let matrix = ProjectionMatrix::new();
    let result = sample_tool_result("shell");
    let translated = matrix
        .translate_tool_result("openai", "anthropic", &result)
        .unwrap();
    assert_eq!(translated.tool_name, "Bash");
    assert_eq!(translated.output, result.output);
}

#[test]
fn translate_tool_result_openai_to_gemini() {
    let matrix = ProjectionMatrix::new();
    let result = sample_tool_result("file_read");
    let translated = matrix
        .translate_tool_result("openai", "gemini", &result)
        .unwrap();
    assert_eq!(translated.tool_name, "readFile");
}

#[test]
fn translate_event_tool_call_openai_to_anthropic() {
    let matrix = ProjectionMatrix::new();
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "shell".into(),
            tool_use_id: Some("tc-10".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"command": "ls"}),
        },
        ext: None,
    };
    let translated = matrix
        .translate_event("openai", "anthropic", &event)
        .unwrap();
    match &translated.kind {
        AgentEventKind::ToolCall { tool_name, .. } => {
            assert_eq!(tool_name, "Bash");
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn translate_event_tool_result_openai_to_gemini() {
    let matrix = ProjectionMatrix::new();
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolResult {
            tool_name: "file_write".into(),
            tool_use_id: Some("tc-11".into()),
            output: serde_json::json!({"ok": true}),
            is_error: false,
        },
        ext: None,
    };
    let translated = matrix.translate_event("openai", "gemini", &event).unwrap();
    match &translated.kind {
        AgentEventKind::ToolResult {
            tool_name,
            is_error,
            ..
        } => {
            assert_eq!(tool_name, "writeFile");
            assert!(!*is_error);
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn translate_event_assistant_message_openai_passthrough() {
    let matrix = ProjectionMatrix::new();
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "Done!".into(),
        },
        ext: None,
    };
    let translated = matrix
        .translate_event("openai", "anthropic", &event)
        .unwrap();
    match &translated.kind {
        AgentEventKind::AssistantMessage { text } => assert_eq!(text, "Done!"),
        other => panic!("expected AssistantMessage, got {other:?}"),
    }
}

#[test]
fn event_mapping_openai_to_anthropic() {
    let matrix = ProjectionMatrix::new();
    let mapping = matrix.event_mapping("openai", "anthropic").unwrap();
    assert_eq!(
        mapping.kind_map.get("response.created").unwrap(),
        "message_start"
    );
    assert_eq!(mapping.kind_map.get("function_call").unwrap(), "tool_use");
}

#[test]
fn event_mapping_openai_to_gemini() {
    let matrix = ProjectionMatrix::new();
    let mapping = matrix.event_mapping("openai", "gemini").unwrap();
    assert_eq!(
        mapping.kind_map.get("response.created").unwrap(),
        "generate_content_start"
    );
    assert_eq!(
        mapping.kind_map.get("function_call_output").unwrap(),
        "function_response"
    );
}

#[test]
fn event_mapping_abp_to_openai() {
    let matrix = ProjectionMatrix::new();
    let mapping = matrix.event_mapping("abp", "openai").unwrap();
    assert_eq!(
        mapping.kind_map.get("run_started").unwrap(),
        "response.created"
    );
    assert_eq!(
        mapping.kind_map.get("tool_result").unwrap(),
        "function_call_output"
    );
}

#[test]
fn full_chain_abp_openai_anthropic_gemini_abp() {
    let matrix = ProjectionMatrix::new();
    let call = sample_tool_call("glob");

    let step1 = matrix.translate_tool_call("abp", "openai", &call).unwrap();
    assert_eq!(step1.tool_name, "file_search");

    let step2 = matrix
        .translate_tool_call("openai", "anthropic", &step1)
        .unwrap();
    assert_eq!(step2.tool_name, "Glob");

    let step3 = matrix
        .translate_tool_call("anthropic", "gemini", &step2)
        .unwrap();
    assert_eq!(step3.tool_name, "searchFiles");

    let step4 = matrix.translate_tool_call("gemini", "abp", &step3).unwrap();
    assert_eq!(step4.tool_name, "glob");
    assert_eq!(step4.input, call.input);
}

#[test]
fn empty_tool_name_passthrough() {
    let matrix = ProjectionMatrix::new();
    let call = ToolCall {
        tool_name: String::new(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: serde_json::json!({}),
    };
    let translated = matrix.translate_tool_call("abp", "openai", &call).unwrap();
    assert_eq!(translated.tool_name, "");
}

#[test]
fn tool_result_error_flag_preserved() {
    let matrix = ProjectionMatrix::new();
    let result = ToolResult {
        tool_name: "shell".into(),
        tool_use_id: Some("tc-err".into()),
        output: serde_json::json!({"error": "command not found"}),
        is_error: true,
    };
    let translated = matrix
        .translate_tool_result("openai", "abp", &result)
        .unwrap();
    assert_eq!(translated.tool_name, "bash");
    assert!(translated.is_error);
}

#[test]
fn translate_event_preserves_ext_field() {
    let matrix = ProjectionMatrix::new();
    let mut ext = std::collections::BTreeMap::new();
    ext.insert("custom".to_string(), serde_json::json!("data"));
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "file_read".into(),
            tool_use_id: Some("tc-ext".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({}),
        },
        ext: Some(ext.clone()),
    };
    let translated = matrix
        .translate_event("openai", "anthropic", &event)
        .unwrap();
    assert_eq!(translated.ext, Some(ext));
}

#[test]
fn translate_event_preserves_timestamp() {
    let matrix = ProjectionMatrix::new();
    let now = Utc::now();
    let event = AgentEvent {
        ts: now,
        kind: AgentEventKind::RunStarted {
            message: "starting".into(),
        },
        ext: None,
    };
    let translated = matrix.translate_event("abp", "openai", &event).unwrap();
    assert_eq!(translated.ts, now);
}

#[test]
fn tool_translation_table_openai_to_gemini() {
    let matrix = ProjectionMatrix::new();
    let tt = matrix.tool_translation("openai", "gemini").unwrap();
    assert_eq!(tt.name_map.get("shell").unwrap(), "executeCommand");
    assert_eq!(tt.name_map.get("file_search").unwrap(), "searchFiles");
}

#[test]
fn has_translation_openai_pairs() {
    let matrix = ProjectionMatrix::new();
    assert!(matrix.has_translation("openai", "abp"));
    assert!(matrix.has_translation("abp", "openai"));
    assert!(matrix.has_translation("openai", "anthropic"));
    assert!(matrix.has_translation("openai", "gemini"));
}

#[test]
fn has_translation_known_pair() {
    let matrix = ProjectionMatrix::new();
    assert!(matrix.has_translation("abp", "openai"));
    assert!(matrix.has_translation("openai", "abp"));
    assert!(matrix.has_translation("anthropic", "gemini"));
}

#[test]
fn has_translation_identity() {
    let matrix = ProjectionMatrix::new();
    assert!(matrix.has_translation("abp", "abp"));
    assert!(matrix.has_translation("openai", "openai"));
}

#[test]
fn has_translation_unknown_dialect() {
    let matrix = ProjectionMatrix::new();
    assert!(!matrix.has_translation("abp", "unknown"));
    assert!(!matrix.has_translation("unknown", "abp"));
    assert!(!matrix.has_translation("unknown", "unknown"));
}

#[test]
fn tool_name_mapping_unmapped_passthrough() {
    let matrix = ProjectionMatrix::new();
    let call = sample_tool_call("custom_tool_xyz");
    let translated = matrix.translate_tool_call("abp", "openai", &call).unwrap();
    // Unmapped tool names pass through unchanged.
    assert_eq!(translated.tool_name, "custom_tool_xyz");
}

#[test]
fn event_kind_mapping_exists() {
    let matrix = ProjectionMatrix::new();
    let mapping = matrix.event_mapping("abp", "openai").unwrap();
    assert_eq!(mapping.kind_map.get("tool_call").unwrap(), "function_call");
    assert_eq!(
        mapping.kind_map.get("run_started").unwrap(),
        "response.created"
    );
}

#[test]
fn event_kind_mapping_anthropic() {
    let matrix = ProjectionMatrix::new();
    let mapping = matrix.event_mapping("abp", "anthropic").unwrap();
    assert_eq!(mapping.kind_map.get("tool_call").unwrap(), "tool_use");
    assert_eq!(
        mapping.kind_map.get("run_started").unwrap(),
        "message_start"
    );
}

#[test]
fn multiple_sequential_translations() {
    let matrix = ProjectionMatrix::new();
    let call = sample_tool_call("read_file");

    // ABP → OpenAI → Anthropic → Gemini → ABP
    let step1 = matrix.translate_tool_call("abp", "openai", &call).unwrap();
    assert_eq!(step1.tool_name, "file_read");

    let step2 = matrix
        .translate_tool_call("openai", "anthropic", &step1)
        .unwrap();
    assert_eq!(step2.tool_name, "Read");

    let step3 = matrix
        .translate_tool_call("anthropic", "gemini", &step2)
        .unwrap();
    assert_eq!(step3.tool_name, "readFile");

    let step4 = matrix.translate_tool_call("gemini", "abp", &step3).unwrap();
    assert_eq!(step4.tool_name, "read_file");
    assert_eq!(step4.input, call.input);
}

#[test]
fn cross_vendor_tool_call_translation() {
    let matrix = ProjectionMatrix::new();
    let call = ToolCall {
        tool_name: "shell".into(),
        tool_use_id: Some("tc-5".into()),
        parent_tool_use_id: None,
        input: serde_json::json!({"command": "ls"}),
    };
    let translated = matrix
        .translate_tool_call("openai", "anthropic", &call)
        .unwrap();
    assert_eq!(translated.tool_name, "Bash");
}

#[test]
fn tool_translation_table_accessible() {
    let matrix = ProjectionMatrix::new();
    let tt = matrix.tool_translation("abp", "gemini").unwrap();
    assert_eq!(tt.name_map.get("bash").unwrap(), "executeCommand");
    assert_eq!(tt.name_map.get("glob").unwrap(), "searchFiles");
}
