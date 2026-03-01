// SPDX-License-Identifier: MIT OR Apache-2.0
//! End-to-end dialect mapping integration tests.
//!
//! Verifies that each vendor SDK can:
//!   1. Map a [`WorkOrder`] to a vendor-specific request.
//!   2. Map a mock vendor response back to valid [`AgentEvent`]s.
//!   3. Produce consistent output across repeated runs (symmetry).
//!
//! Also exercises the [`ProjectionMatrix`] translation pipeline.

use abp_core::{AgentEventKind, ContextPacket, ContextSnippet, WorkOrderBuilder};
use abp_integrations::projection::{Dialect, ProjectionMatrix, translate};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn simple_work_order() -> abp_core::WorkOrder {
    WorkOrderBuilder::new("Refactor the authentication module").build()
}

fn work_order_with_context() -> abp_core::WorkOrder {
    WorkOrderBuilder::new("Fix the login bug")
        .context(ContextPacket {
            files: vec!["src/auth.rs".into()],
            snippets: vec![ContextSnippet {
                name: "error log".into(),
                content: "panic at auth.rs:42".into(),
            }],
        })
        .build()
}

// ---------------------------------------------------------------------------
// 1. Per-vendor: map_work_order → verify vendor fields → map_response → events
// ---------------------------------------------------------------------------

mod claude {
    use super::*;
    use abp_claude_sdk::dialect::*;

    #[test]
    fn roundtrip_simple() {
        let wo = simple_work_order();
        let cfg = ClaudeConfig::default();
        let req = map_work_order(&wo, &cfg);

        assert!(!req.model.is_empty(), "model must be set");
        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.messages[0].role, "user");
        assert!(
            req.messages[0]
                .content
                .contains("Refactor the authentication module")
        );

        let resp = ClaudeResponse {
            id: "msg_int_1".into(),
            model: req.model.clone(),
            role: "assistant".into(),
            content: vec![
                ClaudeContentBlock::Text {
                    text: "I'll refactor the auth module.".into(),
                },
                ClaudeContentBlock::ToolUse {
                    id: "tu_1".into(),
                    name: "read_file".into(),
                    input: serde_json::json!({"path": "src/auth.rs"}),
                },
            ],
            stop_reason: Some("tool_use".into()),
            usage: Some(ClaudeUsage {
                input_tokens: 100,
                output_tokens: 50,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            }),
        };
        let events = map_response(&resp);
        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0].kind,
            AgentEventKind::AssistantMessage { .. }
        ));
        assert!(matches!(&events[1].kind, AgentEventKind::ToolCall { .. }));
    }

    #[test]
    fn context_snippets_included() {
        let wo = work_order_with_context();
        let cfg = ClaudeConfig::default();
        let req = map_work_order(&wo, &cfg);

        assert!(req.messages[0].content.contains("error log"));
        assert!(req.messages[0].content.contains("panic at auth.rs:42"));
    }
}

mod codex {
    use super::*;
    use abp_codex_sdk::dialect::*;

    #[test]
    fn roundtrip_simple() {
        let wo = simple_work_order();
        let cfg = CodexConfig::default();
        let req = map_work_order(&wo, &cfg);

        assert!(!req.model.is_empty());
        assert_eq!(req.input.len(), 1);
        match &req.input[0] {
            CodexInputItem::Message { role, content } => {
                assert_eq!(role, "user");
                assert!(content.contains("Refactor the authentication module"));
            }
        }

        let resp = CodexResponse {
            id: "resp_int_1".into(),
            model: req.model.clone(),
            output: vec![
                CodexOutputItem::Message {
                    role: "assistant".into(),
                    content: vec![CodexContentPart::OutputText {
                        text: "Refactoring complete.".into(),
                    }],
                },
                CodexOutputItem::FunctionCall {
                    id: "fc_1".into(),
                    name: "shell".into(),
                    arguments: r#"{"command":"cargo test"}"#.into(),
                },
            ],
            usage: Some(CodexUsage {
                input_tokens: 80,
                output_tokens: 40,
                total_tokens: 120,
            }),
        };
        let events = map_response(&resp);
        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0].kind,
            AgentEventKind::AssistantMessage { .. }
        ));
        assert!(
            matches!(&events[1].kind, AgentEventKind::ToolCall { tool_name, .. } if tool_name == "shell")
        );
    }

    #[test]
    fn context_snippets_included() {
        let wo = work_order_with_context();
        let cfg = CodexConfig::default();
        let req = map_work_order(&wo, &cfg);

        match &req.input[0] {
            CodexInputItem::Message { content, .. } => {
                assert!(content.contains("error log"));
                assert!(content.contains("panic at auth.rs:42"));
            }
        }
    }
}

mod gemini {
    use super::*;
    use abp_gemini_sdk::dialect::*;

    #[test]
    fn roundtrip_simple() {
        let wo = simple_work_order();
        let cfg = GeminiConfig::default();
        let req = map_work_order(&wo, &cfg);

        assert!(!req.model.is_empty());
        assert_eq!(req.contents.len(), 1);
        assert_eq!(req.contents[0].role, "user");
        match &req.contents[0].parts[0] {
            GeminiPart::Text(t) => {
                assert!(t.contains("Refactor the authentication module"));
            }
            other => panic!("expected Text part, got {other:?}"),
        }

        let resp = GeminiResponse {
            candidates: vec![GeminiCandidate {
                content: GeminiContent {
                    role: "model".into(),
                    parts: vec![
                        GeminiPart::Text("Here is the refactored code.".into()),
                        GeminiPart::FunctionCall {
                            name: "write_file".into(),
                            args: serde_json::json!({"path": "src/auth.rs", "content": "// new"}),
                        },
                    ],
                },
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
                citation_metadata: None,
            }],
            usage_metadata: Some(GeminiUsageMetadata {
                prompt_token_count: 90,
                candidates_token_count: 45,
                total_token_count: 135,
            }),
        };
        let events = map_response(&resp);
        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0].kind,
            AgentEventKind::AssistantMessage { .. }
        ));
        assert!(
            matches!(&events[1].kind, AgentEventKind::ToolCall { tool_name, .. } if tool_name == "write_file")
        );
    }

    #[test]
    fn context_snippets_included() {
        let wo = work_order_with_context();
        let cfg = GeminiConfig::default();
        let req = map_work_order(&wo, &cfg);

        match &req.contents[0].parts[0] {
            GeminiPart::Text(t) => {
                assert!(t.contains("error log"));
                assert!(t.contains("panic at auth.rs:42"));
            }
            other => panic!("expected Text part, got {other:?}"),
        }
    }
}

mod kimi {
    use super::*;
    use abp_kimi_sdk::dialect::*;

    #[test]
    fn roundtrip_simple() {
        let wo = simple_work_order();
        let cfg = KimiConfig::default();
        let req = map_work_order(&wo, &cfg);

        assert!(!req.model.is_empty());
        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.messages[0].role, "user");
        assert!(
            req.messages[0]
                .content
                .contains("Refactor the authentication module")
        );

        let resp = KimiResponse {
            id: "cmpl_int_1".into(),
            model: req.model.clone(),
            choices: vec![KimiChoice {
                index: 0,
                message: KimiResponseMessage {
                    role: "assistant".into(),
                    content: Some("Auth module refactored.".into()),
                    tool_calls: Some(vec![KimiToolCall {
                        id: "call_1".into(),
                        call_type: "function".into(),
                        function: KimiFunctionCall {
                            name: "read_file".into(),
                            arguments: r#"{"path":"src/auth.rs"}"#.into(),
                        },
                    }]),
                },
                finish_reason: Some("tool_calls".into()),
            }],
            usage: Some(KimiUsage {
                prompt_tokens: 70,
                completion_tokens: 35,
                total_tokens: 105,
            }),
        };
        let events = map_response(&resp);
        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0].kind,
            AgentEventKind::AssistantMessage { .. }
        ));
        assert!(
            matches!(&events[1].kind, AgentEventKind::ToolCall { tool_name, .. } if tool_name == "read_file")
        );
    }

    #[test]
    fn context_snippets_included() {
        let wo = work_order_with_context();
        let cfg = KimiConfig::default();
        let req = map_work_order(&wo, &cfg);

        assert!(req.messages[0].content.contains("error log"));
        assert!(req.messages[0].content.contains("panic at auth.rs:42"));
    }
}

// ---------------------------------------------------------------------------
// 2. Full pipeline: WorkOrder → ProjectionMatrix.translate → vendor JSON
// ---------------------------------------------------------------------------

#[test]
fn projection_abp_to_claude_has_correct_structure() {
    let wo = simple_work_order();
    let val = translate(Dialect::Abp, Dialect::Claude, &wo).unwrap();
    let obj = val.as_object().expect("should be JSON object");

    assert!(obj.contains_key("model"));
    assert!(obj.contains_key("max_tokens"));
    assert!(obj.contains_key("messages"));

    let messages = obj["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["role"], "user");
    assert!(
        messages[0]["content"]
            .as_str()
            .unwrap()
            .contains("Refactor")
    );
}

#[test]
fn projection_abp_to_gemini_has_correct_structure() {
    let wo = simple_work_order();
    let val = translate(Dialect::Abp, Dialect::Gemini, &wo).unwrap();
    let obj = val.as_object().expect("should be JSON object");

    assert!(obj.contains_key("model"));
    assert!(obj.contains_key("contents"));
    assert!(obj.contains_key("generation_config"));

    let contents = obj["contents"].as_array().unwrap();
    assert_eq!(contents.len(), 1);
    assert_eq!(contents[0]["role"], "user");
}

#[test]
fn projection_abp_to_codex_has_correct_structure() {
    let wo = simple_work_order();
    let val = translate(Dialect::Abp, Dialect::Codex, &wo).unwrap();
    let obj = val.as_object().expect("should be JSON object");

    assert!(obj.contains_key("model"));
    assert!(obj.contains_key("input"));
    assert!(obj.contains_key("max_output_tokens"));

    let input = obj["input"].as_array().unwrap();
    assert_eq!(input.len(), 1);
    assert_eq!(input[0]["role"], "user");
}

#[test]
fn projection_abp_to_kimi_has_correct_structure() {
    let wo = simple_work_order();
    let val = translate(Dialect::Abp, Dialect::Kimi, &wo).unwrap();
    let obj = val.as_object().expect("should be JSON object");

    assert!(obj.contains_key("model"));
    assert!(obj.contains_key("messages"));
    assert!(obj.contains_key("max_tokens"));

    let messages = obj["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["role"], "user");
}

#[test]
fn projection_matrix_method_matches_free_fn_for_all_vendors() {
    let wo = simple_work_order();
    let matrix = ProjectionMatrix::new();

    for &dialect in &[
        Dialect::Claude,
        Dialect::Codex,
        Dialect::Gemini,
        Dialect::Kimi,
    ] {
        let method_val = matrix.translate(Dialect::Abp, dialect, &wo).unwrap();
        let free_val = translate(Dialect::Abp, dialect, &wo).unwrap();
        assert_eq!(method_val, free_val, "mismatch for {dialect:?}");
    }
}

// ---------------------------------------------------------------------------
// 3. Symmetry: same WorkOrder produces identical output across runs
// ---------------------------------------------------------------------------

#[test]
fn translation_is_deterministic_across_runs() {
    // Build the WorkOrder once (fixed ID via clone) and translate twice.
    let wo = simple_work_order();

    for &dialect in &[
        Dialect::Claude,
        Dialect::Codex,
        Dialect::Gemini,
        Dialect::Kimi,
    ] {
        let first = translate(Dialect::Abp, dialect, &wo).unwrap();
        let second = translate(Dialect::Abp, dialect, &wo).unwrap();
        assert_eq!(
            first, second,
            "non-deterministic translation for {dialect:?}"
        );
    }
}

#[test]
fn identity_translation_is_deterministic() {
    let wo = simple_work_order();

    for &dialect in Dialect::ALL {
        let first = translate(dialect, dialect, &wo).unwrap();
        let second = translate(dialect, dialect, &wo).unwrap();
        assert_eq!(first, second, "identity not stable for {dialect:?}");
    }
}

#[test]
fn context_preserved_in_all_vendor_translations() {
    let wo = work_order_with_context();

    for &dialect in &[
        Dialect::Claude,
        Dialect::Codex,
        Dialect::Gemini,
        Dialect::Kimi,
    ] {
        let val = translate(Dialect::Abp, dialect, &wo).unwrap();
        let json = val.to_string();
        assert!(
            json.contains("error log") || json.contains("panic at auth.rs:42"),
            "context snippet missing in {dialect:?} translation"
        );
    }
}
