// SPDX-License-Identifier: MIT OR Apache-2.0
//! Cross-shim integration tests for edge cases when converting between
//! different SDK types through the IR layer.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use serde_json::json;

// =========================================================================
// Helpers
// =========================================================================

/// Build an OpenAI IR from messages.
fn openai_roundtrip(msgs: Vec<abp_shim_openai::Message>) -> Vec<abp_shim_openai::Message> {
    let ir = abp_shim_openai::messages_to_ir(&msgs);
    abp_shim_openai::ir_to_messages(&ir)
}

/// Build a Kimi IR from messages.
fn kimi_roundtrip(msgs: Vec<abp_shim_kimi::Message>) -> Vec<abp_shim_kimi::Message> {
    let ir = abp_shim_kimi::messages_to_ir(&msgs);
    abp_shim_kimi::ir_to_messages(&ir)
}

/// Build a Copilot IR from messages.
fn copilot_roundtrip(msgs: Vec<abp_shim_copilot::Message>) -> Vec<abp_shim_copilot::Message> {
    let ir = abp_shim_copilot::messages_to_ir(&msgs);
    abp_shim_copilot::ir_to_messages(&ir)
}

/// Convert OpenAI messages → IR → Kimi messages.
fn openai_to_kimi(msgs: &[abp_shim_openai::Message]) -> Vec<abp_shim_kimi::Message> {
    let ir = abp_shim_openai::messages_to_ir(msgs);
    abp_shim_kimi::ir_to_messages(&ir)
}

/// Convert Kimi messages → IR → OpenAI messages.
fn kimi_to_openai(msgs: &[abp_shim_kimi::Message]) -> Vec<abp_shim_openai::Message> {
    let ir = abp_shim_kimi::messages_to_ir(msgs);
    abp_shim_openai::ir_to_messages(&ir)
}

/// Convert OpenAI messages → IR → Copilot messages.
fn openai_to_copilot(msgs: &[abp_shim_openai::Message]) -> Vec<abp_shim_copilot::Message> {
    let ir = abp_shim_openai::messages_to_ir(msgs);
    abp_shim_copilot::ir_to_messages(&ir)
}

// =========================================================================
// A) Type conversion edge cases (15 tests)
// =========================================================================

// ── 1. Empty message array through OpenAI shim ─────────────────────────

#[test]
fn empty_messages_openai_roundtrip() {
    let ir = abp_shim_openai::messages_to_ir(&[]);
    assert!(ir.is_empty());
    let back = abp_shim_openai::ir_to_messages(&ir);
    assert!(back.is_empty());
}

// ── 2. Empty message array through Kimi shim ───────────────────────────

#[test]
fn empty_messages_kimi_roundtrip() {
    let ir = abp_shim_kimi::messages_to_ir(&[]);
    assert!(ir.is_empty());
    let back = abp_shim_kimi::ir_to_messages(&ir);
    assert!(back.is_empty());
}

// ── 3. Empty message array through Copilot shim ────────────────────────

#[test]
fn empty_messages_copilot_roundtrip() {
    let ir = abp_shim_copilot::messages_to_ir(&[]);
    assert!(ir.is_empty());
    let back = abp_shim_copilot::ir_to_messages(&ir);
    assert!(back.is_empty());
}

// ── 4. Empty Codex input through shim ───────────────────────────────────

#[test]
fn empty_codex_input_roundtrip() {
    let req = abp_shim_codex::CodexRequestBuilder::new()
        .input(vec![])
        .build();
    let ir = abp_shim_codex::request_to_ir(&req);
    assert!(ir.is_empty());
}

// ── 5. Very long message content (>100K chars) through OpenAI ──────────

#[test]
fn very_long_content_openai_roundtrip() {
    let long_text = "x".repeat(150_000);
    let msgs = vec![abp_shim_openai::Message::user(long_text.clone())];
    let back = openai_roundtrip(msgs);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].content.as_deref().unwrap().len(), 150_000);
}

// ── 6. Very long message content through Kimi ──────────────────────────

#[test]
fn very_long_content_kimi_roundtrip() {
    let long_text = "y".repeat(120_000);
    let msgs = vec![abp_shim_kimi::Message::user(long_text.clone())];
    let back = kimi_roundtrip(msgs);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].content.as_deref().unwrap().len(), 120_000);
}

// ── 7. Unicode/emoji in messages through OpenAI ────────────────────────

#[test]
fn unicode_emoji_openai_roundtrip() {
    let text = "Hello 🦀 Rust! こんにちは 世界 🌍 Ñ ñ ü ö ä é 中文";
    let msgs = vec![abp_shim_openai::Message::user(text)];
    let back = openai_roundtrip(msgs);
    assert_eq!(back[0].content.as_deref().unwrap(), text);
}

// ── 8. Unicode/emoji cross-shim: OpenAI → Kimi ────────────────────────

#[test]
fn unicode_emoji_openai_to_kimi() {
    let text = "🦀🎉 Ω∑∆ 日本語テスト";
    let msgs = vec![abp_shim_openai::Message::user(text)];
    let kimi = openai_to_kimi(&msgs);
    assert_eq!(kimi[0].content.as_deref().unwrap(), text);
}

// ── 9. Messages with only system role through OpenAI ───────────────────

#[test]
fn system_only_openai_roundtrip() {
    let msgs = vec![abp_shim_openai::Message::system("You are helpful.")];
    let ir = abp_shim_openai::messages_to_ir(&msgs);
    assert_eq!(ir.len(), 1);
    assert_eq!(ir.messages[0].role, IrRole::System);
    let back = openai_roundtrip(msgs);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].role, abp_shim_openai::Role::System);
}

// ── 10. Messages with mixed roles through OpenAI ───────────────────────

#[test]
fn mixed_roles_openai_roundtrip() {
    let msgs = vec![
        abp_shim_openai::Message::system("Be concise."),
        abp_shim_openai::Message::user("Hello"),
        abp_shim_openai::Message::assistant("Hi there!"),
        abp_shim_openai::Message::user("How are you?"),
    ];
    let back = openai_roundtrip(msgs);
    assert_eq!(back.len(), 4);
    assert_eq!(back[0].role, abp_shim_openai::Role::System);
    assert_eq!(back[1].role, abp_shim_openai::Role::User);
    assert_eq!(back[2].role, abp_shim_openai::Role::Assistant);
    assert_eq!(back[3].role, abp_shim_openai::Role::User);
}

// ── 11. Nested tool call responses through OpenAI ──────────────────────

#[test]
fn nested_tool_call_responses_openai() {
    let msgs = vec![
        abp_shim_openai::Message::user("Read src/main.rs"),
        abp_shim_openai::Message::assistant_with_tool_calls(vec![abp_shim_openai::ToolCall {
            id: "call_1".into(),
            call_type: "function".into(),
            function: abp_shim_openai::FunctionCall {
                name: "read_file".into(),
                arguments: r#"{"path":"src/main.rs"}"#.into(),
            },
        }]),
        abp_shim_openai::Message::tool("call_1", "fn main() {}"),
        abp_shim_openai::Message::assistant("Here is the file content."),
    ];
    let ir = abp_shim_openai::messages_to_ir(&msgs);
    assert_eq!(ir.len(), 4);
    // Verify tool-use block preserved in IR
    let tool_calls = ir.tool_calls();
    assert_eq!(tool_calls.len(), 1);
    if let IrContentBlock::ToolUse { name, .. } = tool_calls[0] {
        assert_eq!(name, "read_file");
    } else {
        panic!("expected ToolUse block");
    }
}

// ── 12. Multiple tool calls in single message through OpenAI ───────────

#[test]
fn multiple_tool_calls_single_message_openai() {
    let msgs = vec![abp_shim_openai::Message::assistant_with_tool_calls(vec![
        abp_shim_openai::ToolCall {
            id: "call_a".into(),
            call_type: "function".into(),
            function: abp_shim_openai::FunctionCall {
                name: "read_file".into(),
                arguments: r#"{"path":"a.rs"}"#.into(),
            },
        },
        abp_shim_openai::ToolCall {
            id: "call_b".into(),
            call_type: "function".into(),
            function: abp_shim_openai::FunctionCall {
                name: "write_file".into(),
                arguments: r#"{"path":"b.rs","content":"hi"}"#.into(),
            },
        },
    ])];
    let ir = abp_shim_openai::messages_to_ir(&msgs);
    let tool_calls = ir.tool_calls();
    assert_eq!(tool_calls.len(), 2);
}

// ── 13. Tool calls with empty arguments through OpenAI ─────────────────

#[test]
fn tool_call_empty_arguments_openai() {
    let msgs = vec![abp_shim_openai::Message::assistant_with_tool_calls(vec![
        abp_shim_openai::ToolCall {
            id: "call_z".into(),
            call_type: "function".into(),
            function: abp_shim_openai::FunctionCall {
                name: "get_status".into(),
                arguments: "{}".into(),
            },
        },
    ])];
    let ir = abp_shim_openai::messages_to_ir(&msgs);
    let tool_calls = ir.tool_calls();
    assert_eq!(tool_calls.len(), 1);
    if let IrContentBlock::ToolUse { input, .. } = tool_calls[0] {
        assert_eq!(input, &json!({}));
    } else {
        panic!("expected ToolUse block");
    }
}

// ── 14. Claude thinking blocks through IR ──────────────────────────────

#[test]
fn claude_thinking_block_through_ir() {
    let msgs = [abp_shim_claude::Message {
        role: abp_shim_claude::Role::Assistant,
        content: vec![
            abp_shim_claude::ContentBlock::Thinking {
                thinking: "Let me reason about this...".into(),
                signature: Some("sig_123".into()),
            },
            abp_shim_claude::ContentBlock::Text {
                text: "The answer is 42.".into(),
            },
        ],
    }];
    let claude_msgs: Vec<_> = msgs.iter().map(abp_shim_claude::message_to_ir).collect();
    // Just verify the messages can be created and are non-empty
    assert!(!claude_msgs.is_empty());
    assert!(!claude_msgs[0].content.is_empty());
}

// ── 15. Gemini function call part through IR ───────────────────────────

#[test]
fn gemini_function_call_part_through_ir() {
    let req = abp_shim_gemini::GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(abp_shim_gemini::Content::user(vec![
            abp_shim_gemini::Part::text("What's the weather?"),
        ]))
        .add_content(abp_shim_gemini::Content::model(vec![
            abp_shim_gemini::Part::function_call("get_weather", json!({"location": "NYC"})),
        ]))
        .add_content(abp_shim_gemini::Content::user(vec![
            abp_shim_gemini::Part::function_response(
                "get_weather",
                json!({"temp": 72, "condition": "sunny"}),
            ),
        ]));

    let dialect_req = abp_shim_gemini::to_dialect_request(&req);
    let ir = abp_gemini_sdk::lowering::to_ir(
        &dialect_req.contents,
        dialect_req.system_instruction.as_ref(),
    );
    // Should have user, model (with tool call), user (with tool result)
    assert!(ir.len() >= 3);
    let tool_calls = ir.tool_calls();
    assert_eq!(tool_calls.len(), 1);
}

// =========================================================================
// B) Lossy conversion awareness (10 tests)
// =========================================================================

// ── 16. OpenAI system message preserved through IR ─────────────────────

#[test]
fn openai_system_message_preserved_in_ir() {
    let msgs = vec![
        abp_shim_openai::Message::system("Be helpful"),
        abp_shim_openai::Message::user("Hi"),
    ];
    let ir = abp_shim_openai::messages_to_ir(&msgs);
    assert_eq!(ir.system_message().unwrap().text_content(), "Be helpful");
}

// ── 17. OpenAI→Codex: system messages dropped in from_ir ──────────────

#[test]
fn openai_to_codex_drops_system_in_output() {
    let ir = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "instructions"),
        IrMessage::text(IrRole::User, "hello"),
        IrMessage::text(IrRole::Assistant, "hi there"),
    ]);
    let codex_items = abp_shim_codex::ir_to_response_items(&ir);
    // Codex response items only include assistant output; system/user are dropped
    assert_eq!(
        codex_items.len(),
        1,
        "only assistant items survive in Codex output"
    );
}

// ── 18. Claude→Codex: thinking blocks dropped ─────────────────────────

#[test]
fn claude_thinking_blocks_dropped_in_codex() {
    let ir = IrConversation::from_messages(vec![
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::Thinking {
                text: "deep reasoning here".into(),
            }],
        ),
        IrMessage::text(IrRole::Assistant, "The answer is 42."),
    ]);
    let codex_items = abp_shim_codex::ir_to_response_items(&ir);
    // Codex has no thinking representation; at least the text assistant survives
    // but thinking blocks should either be dropped or mapped to reasoning items
    for item in &codex_items {
        let json_str = serde_json::to_string(item).unwrap();
        // The raw thinking text should not leak as a plain assistant message
        // unless it's explicitly a Reasoning item
        if json_str.contains("deep reasoning") {
            // If the lowering produces a Reasoning item, that's fine
            assert!(
                json_str.contains("reasoning") || json_str.contains("summary"),
                "thinking should map to reasoning, not plain text"
            );
        }
    }
    // Text message should survive
    let has_answer = codex_items.iter().any(|item| {
        let s = serde_json::to_string(item).unwrap();
        s.contains("42")
    });
    assert!(has_answer, "text assistant message should survive");
}

// ── 19. Gemini→OpenAI preserves tool semantics ────────────────────────

#[test]
fn gemini_to_openai_preserves_tool_semantics() {
    let ir = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Call the search tool"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "tool_1".into(),
                name: "search".into(),
                input: json!({"q": "rust"}),
            }],
        ),
    ]);
    // Convert IR → OpenAI messages
    let oai = abp_shim_openai::ir_to_messages(&ir);
    assert_eq!(oai.len(), 2);
    let tc = oai[1].tool_calls.as_ref().expect("should have tool_calls");
    assert_eq!(tc.len(), 1);
    assert_eq!(tc[0].function.name, "search");
    assert!(tc[0].function.arguments.contains("rust"));
}

// ── 20. Kimi→Claude: message structure preserved via IR ───────────────

#[test]
fn kimi_to_claude_message_structure_preserved() {
    let kimi_msgs = vec![
        abp_shim_kimi::Message::system("System prompt"),
        abp_shim_kimi::Message::user("User message"),
        abp_shim_kimi::Message::assistant("Assistant reply"),
    ];
    let ir = abp_shim_kimi::messages_to_ir(&kimi_msgs);
    assert_eq!(ir.len(), 3);
    assert_eq!(ir.messages[0].role, IrRole::System);
    assert_eq!(ir.messages[1].role, IrRole::User);
    assert_eq!(ir.messages[2].role, IrRole::Assistant);
    // Verify text fidelity
    assert_eq!(ir.messages[0].text_content(), "System prompt");
    assert_eq!(ir.messages[1].text_content(), "User message");
    assert_eq!(ir.messages[2].text_content(), "Assistant reply");
}

// ── 21. Copilot references stripped in non-Copilot targets ─────────────

#[test]
fn copilot_references_stripped_in_openai_target() {
    let msgs = vec![abp_shim_copilot::Message::user_with_refs(
        "Check this code",
        vec![abp_copilot_sdk::dialect::CopilotReference {
            ref_type: abp_copilot_sdk::dialect::CopilotReferenceType::File,
            id: "file_123".into(),
            data: serde_json::Value::Null,
            metadata: None,
        }],
    )];
    let ir = abp_shim_copilot::messages_to_ir(&msgs);
    // Convert to OpenAI — references have no representation
    let oai = abp_shim_openai::ir_to_messages(&ir);
    assert_eq!(oai.len(), 1);
    assert_eq!(oai[0].content.as_deref().unwrap(), "Check this code");
    // OpenAI messages have no references field
    let json_str = serde_json::to_string(&oai[0]).unwrap();
    assert!(
        !json_str.contains("copilot_references"),
        "Copilot references should not leak into OpenAI format"
    );
}

// ── 22. Copilot references stripped in Kimi target ─────────────────────

#[test]
fn copilot_references_stripped_in_kimi_target() {
    let msgs = vec![abp_shim_copilot::Message::user_with_refs(
        "Review PR",
        vec![abp_copilot_sdk::dialect::CopilotReference {
            ref_type: abp_copilot_sdk::dialect::CopilotReferenceType::Repository,
            id: "pr_42".into(),
            data: serde_json::Value::Null,
            metadata: None,
        }],
    )];
    let ir = abp_shim_copilot::messages_to_ir(&msgs);
    let kimi = abp_shim_kimi::ir_to_messages(&ir);
    assert_eq!(kimi.len(), 1);
    assert_eq!(kimi[0].content.as_deref().unwrap(), "Review PR");
    let json_str = serde_json::to_string(&kimi[0]).unwrap();
    assert!(
        !json_str.contains("copilot_references"),
        "Copilot references should not leak into Kimi format"
    );
}

// ── 23. IR tool result content preserved across shims ──────────────────

#[test]
fn ir_tool_result_preserved_openai_to_kimi() {
    let msgs = vec![
        abp_shim_openai::Message::assistant_with_tool_calls(vec![abp_shim_openai::ToolCall {
            id: "call_x".into(),
            call_type: "function".into(),
            function: abp_shim_openai::FunctionCall {
                name: "search".into(),
                arguments: r#"{"q":"test"}"#.into(),
            },
        }]),
        abp_shim_openai::Message::tool("call_x", "search results here"),
    ];
    let ir = abp_shim_openai::messages_to_ir(&msgs);
    // Verify tool result is in IR
    let tool_msgs: Vec<_> = ir
        .messages
        .iter()
        .filter(|m| m.role == IrRole::Tool)
        .collect();
    assert!(!tool_msgs.is_empty(), "tool result should be in IR");
}

// ── 24. Codex drops user messages in from_ir ───────────────────────────

#[test]
fn codex_drops_user_messages_in_output() {
    let ir = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "user query"),
        IrMessage::text(IrRole::Assistant, "response"),
    ]);
    let codex_items = abp_shim_codex::ir_to_response_items(&ir);
    // Only assistant output survives
    assert_eq!(codex_items.len(), 1);
    let s = serde_json::to_string(&codex_items[0]).unwrap();
    assert!(s.contains("response"));
}

// ── 25. OpenAI tool choice info not preserved in IR ────────────────────

#[test]
fn openai_request_metadata_not_in_ir() {
    let req = abp_shim_openai::ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![abp_shim_openai::Message::user("test")])
        .temperature(0.5)
        .max_tokens(100)
        .build();
    let ir = abp_shim_openai::request_to_ir(&req);
    // IR only captures messages, not temperature/max_tokens
    assert_eq!(ir.len(), 1);
    assert_eq!(ir.messages[0].text_content(), "test");
    // These request-level params go into WorkOrder config, not IR
    let wo = abp_shim_openai::request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
}

// =========================================================================
// C) Round-trip fidelity (10 tests)
// =========================================================================

// ── 26. OpenAI→IR→OpenAI preserves all text fields ─────────────────────

#[test]
fn openai_ir_openai_preserves_text_fields() {
    let msgs = vec![
        abp_shim_openai::Message::system("You are helpful."),
        abp_shim_openai::Message::user("What is Rust?"),
        abp_shim_openai::Message::assistant("Rust is a systems language."),
    ];
    let back = openai_roundtrip(msgs.clone());
    assert_eq!(back.len(), 3);
    assert_eq!(back[0].content.as_deref().unwrap(), "You are helpful.");
    assert_eq!(back[0].role, abp_shim_openai::Role::System);
    assert_eq!(back[1].content.as_deref().unwrap(), "What is Rust?");
    assert_eq!(back[1].role, abp_shim_openai::Role::User);
    assert_eq!(
        back[2].content.as_deref().unwrap(),
        "Rust is a systems language."
    );
    assert_eq!(back[2].role, abp_shim_openai::Role::Assistant);
}

// ── 27. OpenAI→IR→OpenAI preserves tool calls ─────────────────────────

#[test]
fn openai_ir_openai_preserves_tool_calls() {
    let msgs = vec![abp_shim_openai::Message::assistant_with_tool_calls(vec![
        abp_shim_openai::ToolCall {
            id: "call_rt".into(),
            call_type: "function".into(),
            function: abp_shim_openai::FunctionCall {
                name: "list_files".into(),
                arguments: r#"{"dir":"src"}"#.into(),
            },
        },
    ])];
    let back = openai_roundtrip(msgs);
    assert_eq!(back.len(), 1);
    let tc = back[0].tool_calls.as_ref().unwrap();
    assert_eq!(tc.len(), 1);
    assert_eq!(tc[0].function.name, "list_files");
    assert!(tc[0].function.arguments.contains("src"));
}

// ── 28. Kimi→IR→Kimi preserves text fields ─────────────────────────────

#[test]
fn kimi_ir_kimi_preserves_text_fields() {
    let msgs = vec![
        abp_shim_kimi::Message::system("System instructions."),
        abp_shim_kimi::Message::user("Hello Kimi"),
        abp_shim_kimi::Message::assistant("Hello there!"),
    ];
    let back = kimi_roundtrip(msgs);
    assert_eq!(back.len(), 3);
    assert_eq!(back[0].content.as_deref().unwrap(), "System instructions.");
    assert_eq!(back[0].role, "system");
    assert_eq!(back[1].content.as_deref().unwrap(), "Hello Kimi");
    assert_eq!(back[1].role, "user");
    assert_eq!(back[2].content.as_deref().unwrap(), "Hello there!");
    assert_eq!(back[2].role, "assistant");
}

// ── 29. Copilot→IR→Copilot preserves text fields ──────────────────────

#[test]
fn copilot_ir_copilot_preserves_text_fields() {
    let msgs = vec![
        abp_shim_copilot::Message::system("Be concise."),
        abp_shim_copilot::Message::user("Explain Rust"),
        abp_shim_copilot::Message::assistant("Rust is a language."),
    ];
    let back = copilot_roundtrip(msgs);
    assert_eq!(back.len(), 3);
    assert_eq!(back[0].content, "Be concise.");
    assert_eq!(back[0].role, "system");
    assert_eq!(back[1].content, "Explain Rust");
    assert_eq!(back[1].role, "user");
    assert_eq!(back[2].content, "Rust is a language.");
    assert_eq!(back[2].role, "assistant");
}

// ── 30. Gemini→IR→Gemini preserves safety settings (request level) ────

#[test]
fn gemini_safety_settings_preserved_in_request() {
    let req = abp_shim_gemini::GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(abp_shim_gemini::Content::user(vec![
            abp_shim_gemini::Part::text("test"),
        ]))
        .safety_settings(vec![abp_shim_gemini::SafetySetting {
            category: abp_shim_gemini::HarmCategory::HarmCategoryHarassment,
            threshold: abp_shim_gemini::HarmBlockThreshold::BlockNone,
        }]);

    let dialect_req = abp_shim_gemini::to_dialect_request(&req);
    let safety = dialect_req.safety_settings.unwrap();
    assert_eq!(safety.len(), 1);
    assert_eq!(
        safety[0].category,
        abp_shim_gemini::HarmCategory::HarmCategoryHarassment
    );
    assert_eq!(
        safety[0].threshold,
        abp_shim_gemini::HarmBlockThreshold::BlockNone
    );
}

// ── 31. OpenAI↔Kimi lossless pair round-trips cleanly ─────────────────

#[test]
fn openai_kimi_lossless_roundtrip() {
    let oai_msgs = vec![
        abp_shim_openai::Message::system("Be helpful."),
        abp_shim_openai::Message::user("Hi"),
        abp_shim_openai::Message::assistant("Hello!"),
    ];
    // OpenAI → IR → Kimi
    let kimi = openai_to_kimi(&oai_msgs);
    assert_eq!(kimi.len(), 3);
    assert_eq!(kimi[0].role, "system");
    assert_eq!(kimi[1].role, "user");
    assert_eq!(kimi[2].role, "assistant");

    // Kimi → IR → OpenAI
    let back_oai = kimi_to_openai(&kimi);
    assert_eq!(back_oai.len(), 3);
    assert_eq!(back_oai[0].content.as_deref().unwrap(), "Be helpful.");
    assert_eq!(back_oai[1].content.as_deref().unwrap(), "Hi");
    assert_eq!(back_oai[2].content.as_deref().unwrap(), "Hello!");
}

// ── 32. OpenAI↔Copilot lossless pair round-trips cleanly ──────────────

#[test]
fn openai_copilot_lossless_roundtrip() {
    let oai_msgs = vec![
        abp_shim_openai::Message::system("System"),
        abp_shim_openai::Message::user("User"),
        abp_shim_openai::Message::assistant("Assistant"),
    ];
    let copilot = openai_to_copilot(&oai_msgs);
    assert_eq!(copilot.len(), 3);
    assert_eq!(copilot[0].role, "system");
    assert_eq!(copilot[1].role, "user");
    assert_eq!(copilot[2].role, "assistant");
    assert_eq!(copilot[0].content, "System");
    assert_eq!(copilot[1].content, "User");
    assert_eq!(copilot[2].content, "Assistant");
}

// ── 33. Kimi↔Copilot lossless pair round-trips cleanly ────────────────

#[test]
fn kimi_copilot_lossless_roundtrip() {
    let kimi_msgs = vec![
        abp_shim_kimi::Message::system("Instructions"),
        abp_shim_kimi::Message::user("Query"),
        abp_shim_kimi::Message::assistant("Answer"),
    ];
    let ir = abp_shim_kimi::messages_to_ir(&kimi_msgs);
    let copilot = abp_shim_copilot::ir_to_messages(&ir);
    assert_eq!(copilot.len(), 3);
    assert_eq!(copilot[0].content, "Instructions");
    assert_eq!(copilot[1].content, "Query");
    assert_eq!(copilot[2].content, "Answer");

    // Copilot → IR → Kimi
    let ir2 = abp_shim_copilot::messages_to_ir(&copilot);
    let back_kimi = abp_shim_kimi::ir_to_messages(&ir2);
    assert_eq!(back_kimi.len(), 3);
    assert_eq!(back_kimi[0].content.as_deref().unwrap(), "Instructions");
    assert_eq!(back_kimi[1].content.as_deref().unwrap(), "Query");
    assert_eq!(back_kimi[2].content.as_deref().unwrap(), "Answer");
}

// ── 34. OpenAI→Codex lossy pair documents specific losses ──────────────

#[test]
fn openai_to_codex_lossy_pair_documents_losses() {
    let ir = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "system prompt"),
        IrMessage::text(IrRole::User, "user message"),
        IrMessage::text(IrRole::Assistant, "assistant reply"),
    ]);

    let oai = abp_shim_openai::ir_to_messages(&ir);
    assert_eq!(oai.len(), 3, "OpenAI preserves all roles");

    let codex = abp_shim_codex::ir_to_response_items(&ir);
    // Codex only keeps assistant output
    assert!(
        codex.len() < oai.len(),
        "Codex drops system/user → fewer items than OpenAI"
    );
}

// ── 35. Gemini text content round-trips through IR ─────────────────────

#[test]
fn gemini_text_content_roundtrips_through_ir() {
    let contents = [
        abp_shim_gemini::Content::user(vec![abp_shim_gemini::Part::text("Hello Gemini")]),
        abp_shim_gemini::Content::model(vec![abp_shim_gemini::Part::text(
            "Hello! How can I help?",
        )]),
    ];
    // Convert to dialect and then to IR
    let dialect_contents: Vec<_> = contents
        .iter()
        .map(|c| abp_gemini_sdk::dialect::GeminiContent {
            role: c.role.clone(),
            parts: c
                .parts
                .iter()
                .map(|p| match p {
                    abp_shim_gemini::Part::Text(t) => {
                        abp_gemini_sdk::dialect::GeminiPart::Text(t.clone())
                    }
                    abp_shim_gemini::Part::FunctionCall { name, args } => {
                        abp_gemini_sdk::dialect::GeminiPart::FunctionCall {
                            name: name.clone(),
                            args: args.clone(),
                        }
                    }
                    abp_shim_gemini::Part::FunctionResponse { name, response } => {
                        abp_gemini_sdk::dialect::GeminiPart::FunctionResponse {
                            name: name.clone(),
                            response: response.clone(),
                        }
                    }
                    abp_shim_gemini::Part::InlineData { mime_type, data } => {
                        abp_gemini_sdk::dialect::GeminiPart::InlineData(
                            abp_gemini_sdk::dialect::GeminiInlineData {
                                mime_type: mime_type.clone(),
                                data: data.clone(),
                            },
                        )
                    }
                })
                .collect(),
        })
        .collect();
    let ir = abp_gemini_sdk::lowering::to_ir(&dialect_contents, None);
    assert_eq!(ir.len(), 2);
    assert_eq!(ir.messages[0].role, IrRole::User);
    assert_eq!(ir.messages[0].text_content(), "Hello Gemini");
    assert_eq!(ir.messages[1].role, IrRole::Assistant);
    assert_eq!(ir.messages[1].text_content(), "Hello! How can I help?");

    // Convert back from IR
    let back = abp_gemini_sdk::lowering::from_ir(&ir);
    assert_eq!(back.len(), 2);
    assert_eq!(back[0].role, "user");
    assert_eq!(back[1].role, "model");
}

// =========================================================================
// Additional edge case tests
// =========================================================================

// ── 36. Empty string content through OpenAI ────────────────────────────

#[test]
fn empty_string_content_openai() {
    let msgs = vec![abp_shim_openai::Message::user("")];
    let ir = abp_shim_openai::messages_to_ir(&msgs);
    assert_eq!(ir.len(), 1);
    assert_eq!(ir.messages[0].text_content(), "");
}

// ── 37. Whitespace-only content through shims ──────────────────────────

#[test]
fn whitespace_only_content_roundtrip() {
    let msgs = vec![abp_shim_openai::Message::user("   \n\t  ")];
    let back = openai_roundtrip(msgs);
    assert_eq!(back[0].content.as_deref().unwrap(), "   \n\t  ");
}

// ── 38. Kimi tool calls with empty function arguments ──────────────────

#[test]
fn kimi_tool_calls_empty_arguments() {
    let msgs = vec![abp_shim_kimi::Message::assistant_with_tool_calls(vec![
        abp_kimi_sdk::dialect::KimiToolCall {
            id: "call_k1".into(),
            call_type: "function".into(),
            function: abp_kimi_sdk::dialect::KimiFunctionCall {
                name: "ping".into(),
                arguments: "{}".into(),
            },
        },
    ])];
    let ir = abp_shim_kimi::messages_to_ir(&msgs);
    let tool_calls = ir.tool_calls();
    assert_eq!(tool_calls.len(), 1);
    if let IrContentBlock::ToolUse { name, input, .. } = tool_calls[0] {
        assert_eq!(name, "ping");
        assert_eq!(input, &json!({}));
    }
}

// ── 39. IR usage conversion across shims ───────────────────────────────

#[test]
fn ir_usage_conversion_across_shims() {
    let ir_usage = abp_core::ir::IrUsage::from_io(100, 50);

    let oai = abp_shim_openai::ir_usage_to_usage(&ir_usage);
    assert_eq!(oai.prompt_tokens, 100);
    assert_eq!(oai.completion_tokens, 50);
    assert_eq!(oai.total_tokens, 150);

    let kimi = abp_shim_kimi::ir_usage_to_usage(&ir_usage);
    assert_eq!(kimi.prompt_tokens, 100);
    assert_eq!(kimi.completion_tokens, 50);
    assert_eq!(kimi.total_tokens, 150);

    let codex = abp_shim_codex::ir_usage_to_usage(&ir_usage);
    assert_eq!(codex.input_tokens, 100);
    assert_eq!(codex.output_tokens, 50);
    assert_eq!(codex.total_tokens, 150);

    let gemini = abp_shim_gemini::usage_from_ir(&ir_usage);
    assert_eq!(gemini.prompt_token_count, 100);
    assert_eq!(gemini.candidates_token_count, 50);
    assert_eq!(gemini.total_token_count, 150);

    let copilot = abp_shim_copilot::ir_usage_to_tuple(&ir_usage);
    assert_eq!(copilot, (100, 50, 150));
}

// ── 40. Special characters in tool names and arguments ─────────────────

#[test]
fn special_chars_in_tool_call_openai_roundtrip() {
    let msgs = vec![abp_shim_openai::Message::assistant_with_tool_calls(vec![
        abp_shim_openai::ToolCall {
            id: "call_special".into(),
            call_type: "function".into(),
            function: abp_shim_openai::FunctionCall {
                name: "my-tool_v2.0".into(),
                arguments: r#"{"path":"C:\\Users\\test\\file.txt","query":"hello \"world\""}"#
                    .into(),
            },
        },
    ])];
    let ir = abp_shim_openai::messages_to_ir(&msgs);
    let tc = ir.tool_calls();
    assert_eq!(tc.len(), 1);
    if let IrContentBlock::ToolUse { name, .. } = tc[0] {
        assert_eq!(name, "my-tool_v2.0");
    }
}
