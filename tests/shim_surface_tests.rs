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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for SDK shim surface area compliance.
//!
//! Verifies that every shim crate (OpenAI, Claude, Gemini, Codex, Kimi, Copilot)
//! exports the expected types, supports serde roundtrips, request construction,
//! response parsing, IR lowering/raising, streaming, errors, cross-shim
//! compatibility, API surface completeness, model name handling, capability
//! mapping, and default construction.

use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════════
// 1. Each shim exports expected types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_exports_core_types() {
    // Verify the OpenAI shim exports all expected types.
    let _msg = abp_shim_openai::Message::user("hi");
    let _role = abp_shim_openai::Role::User;
    let _err: Option<abp_shim_openai::ShimError> = None;
    let _client = abp_shim_openai::OpenAiClient::new("gpt-4o");
    let _builder = abp_shim_openai::ChatCompletionRequest::builder();
}

#[test]
fn claude_exports_core_types() {
    let _role = abp_shim_claude::Role::User;
    let _block = abp_shim_claude::ContentBlock::Text { text: "hi".into() };
    let _err: Option<abp_shim_claude::ShimError> = None;
    let _client = abp_shim_claude::AnthropicClient::new();
}

#[test]
fn gemini_exports_core_types() {
    let _part = abp_shim_gemini::Part::text("hi");
    let _content = abp_shim_gemini::Content::user(vec![abp_shim_gemini::Part::text("hi")]);
    let _client = abp_shim_gemini::GeminiClient::new("gemini-2.5-flash");
    let _err: Option<abp_shim_gemini::GeminiError> = None;
}

#[test]
fn codex_exports_core_types() {
    let _msg = abp_shim_codex::codex_message("user", "hi");
    let _builder = abp_shim_codex::CodexRequestBuilder::new();
    let _err: Option<abp_shim_codex::ShimError> = None;
    let _client = abp_shim_codex::CodexClient::new("codex-mini-latest");
}

#[test]
fn kimi_exports_core_types() {
    let _msg = abp_shim_kimi::Message::user("hi");
    let _builder = abp_shim_kimi::KimiRequestBuilder::new();
    let _err: Option<abp_shim_kimi::ShimError> = None;
    let _client = abp_shim_kimi::KimiClient::new("moonshot-v1-8k");
}

#[test]
fn copilot_exports_core_types() {
    let _msg = abp_shim_copilot::Message::user("hi");
    let _builder = abp_shim_copilot::CopilotRequestBuilder::new();
    let _err: Option<abp_shim_copilot::ShimError> = None;
    let _client = abp_shim_copilot::CopilotClient::new("gpt-4o");
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Shim type serde roundtrip
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_message_serde_roundtrip() {
    let msg = abp_shim_openai::Message::user("Hello world");
    let json = serde_json::to_string(&msg).unwrap();
    let back: abp_shim_openai::Message = serde_json::from_str(&json).unwrap();
    assert_eq!(back.content.as_deref(), Some("Hello world"));
    assert_eq!(back.role, abp_shim_openai::Role::User);
}

#[test]
fn openai_request_serde_roundtrip() {
    let req = abp_shim_openai::ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![abp_shim_openai::Message::user("test")])
        .build();
    let json = serde_json::to_string(&req).unwrap();
    let back: abp_shim_openai::ChatCompletionRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.model, "gpt-4o");
    assert_eq!(back.messages.len(), 1);
}

#[test]
fn openai_usage_serde_roundtrip() {
    let usage = abp_shim_openai::Usage {
        prompt_tokens: 10,
        completion_tokens: 20,
        total_tokens: 30,
    };
    let json = serde_json::to_string(&usage).unwrap();
    let back: abp_shim_openai::Usage = serde_json::from_str(&json).unwrap();
    assert_eq!(back, usage);
}

#[test]
fn openai_tool_serde_roundtrip() {
    let tool = abp_shim_openai::Tool::function(
        "read_file",
        "Read a file",
        json!({"type": "object", "properties": {"path": {"type": "string"}}}),
    );
    let json = serde_json::to_string(&tool).unwrap();
    let back: abp_shim_openai::Tool = serde_json::from_str(&json).unwrap();
    assert_eq!(back.function.name, "read_file");
}

#[test]
fn claude_message_serde_roundtrip() {
    let msg = abp_shim_claude::Message {
        role: abp_shim_claude::Role::User,
        content: vec![abp_shim_claude::ContentBlock::Text {
            text: "Hello".into(),
        }],
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: abp_shim_claude::Message = serde_json::from_str(&json).unwrap();
    assert_eq!(back.role, abp_shim_claude::Role::User);
    assert_eq!(back.content.len(), 1);
}

#[test]
fn claude_content_block_text_serde() {
    let block = abp_shim_claude::ContentBlock::Text {
        text: "hello".into(),
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: abp_shim_claude::ContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(back, block);
}

#[test]
fn claude_content_block_tool_use_serde() {
    let block = abp_shim_claude::ContentBlock::ToolUse {
        id: "tu_1".into(),
        name: "bash".into(),
        input: json!({"command": "ls"}),
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: abp_shim_claude::ContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(back, block);
}

#[test]
fn claude_usage_serde_roundtrip() {
    let usage = abp_shim_claude::Usage {
        input_tokens: 100,
        output_tokens: 50,
        cache_creation_input_tokens: Some(10),
        cache_read_input_tokens: None,
    };
    let json = serde_json::to_string(&usage).unwrap();
    let back: abp_shim_claude::Usage = serde_json::from_str(&json).unwrap();
    assert_eq!(back.input_tokens, 100);
    assert_eq!(back.output_tokens, 50);
}

#[test]
fn gemini_part_text_serde_roundtrip() {
    let part = abp_shim_gemini::Part::text("Hello");
    let json = serde_json::to_string(&part).unwrap();
    assert!(json.contains("Hello"));
}

#[test]
fn gemini_content_serde_roundtrip() {
    let content = abp_shim_gemini::Content::user(vec![abp_shim_gemini::Part::text("test content")]);
    let json = serde_json::to_string(&content).unwrap();
    let back: abp_shim_gemini::Content = serde_json::from_str(&json).unwrap();
    assert_eq!(back.role, "user");
}

#[test]
fn gemini_usage_metadata_serde_roundtrip() {
    let usage = abp_shim_gemini::UsageMetadata {
        prompt_token_count: 10,
        candidates_token_count: 20,
        total_token_count: 30,
    };
    let json = serde_json::to_string(&usage).unwrap();
    let back: abp_shim_gemini::UsageMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(back, usage);
}

#[test]
fn codex_usage_serde_roundtrip() {
    let usage = abp_shim_codex::Usage {
        input_tokens: 15,
        output_tokens: 25,
        total_tokens: 40,
    };
    let json = serde_json::to_string(&usage).unwrap();
    let back: abp_shim_codex::Usage = serde_json::from_str(&json).unwrap();
    assert_eq!(back, usage);
}

#[test]
fn kimi_message_serde_roundtrip() {
    let msg = abp_shim_kimi::Message::user("test kimi");
    let json = serde_json::to_string(&msg).unwrap();
    let back: abp_shim_kimi::Message = serde_json::from_str(&json).unwrap();
    assert_eq!(back.role, "user");
    assert_eq!(back.content.as_deref(), Some("test kimi"));
}

#[test]
fn kimi_usage_serde_roundtrip() {
    let usage = abp_shim_kimi::Usage {
        prompt_tokens: 5,
        completion_tokens: 10,
        total_tokens: 15,
    };
    let json = serde_json::to_string(&usage).unwrap();
    let back: abp_shim_kimi::Usage = serde_json::from_str(&json).unwrap();
    assert_eq!(back, usage);
}

#[test]
fn copilot_message_serde_roundtrip() {
    let msg = abp_shim_copilot::Message::user("hello copilot");
    let json = serde_json::to_string(&msg).unwrap();
    let back: abp_shim_copilot::Message = serde_json::from_str(&json).unwrap();
    assert_eq!(back.role, "user");
    assert_eq!(back.content, "hello copilot");
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Shim request construction
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_request_builder_constructs_valid_request() {
    let req = abp_shim_openai::ChatCompletionRequest::builder()
        .model("gpt-4-turbo")
        .messages(vec![
            abp_shim_openai::Message::system("You are helpful"),
            abp_shim_openai::Message::user("Hello"),
        ])
        .temperature(0.7)
        .max_tokens(1000)
        .build();
    assert_eq!(req.model, "gpt-4-turbo");
    assert_eq!(req.messages.len(), 2);
    assert_eq!(req.temperature, Some(0.7));
    assert_eq!(req.max_tokens, Some(1000));
}

#[test]
fn openai_request_builder_with_tools() {
    let tool = abp_shim_openai::Tool::function(
        "search",
        "Search the web",
        json!({"type": "object", "properties": {"q": {"type": "string"}}}),
    );
    let req = abp_shim_openai::ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![abp_shim_openai::Message::user("search for rust")])
        .tools(vec![tool])
        .build();
    assert!(req.tools.is_some());
    assert_eq!(req.tools.as_ref().unwrap().len(), 1);
}

#[test]
fn claude_request_construction() {
    let req = abp_shim_claude::MessageRequest {
        model: "claude-sonnet-4-20250514".to_string(),
        max_tokens: 4096,
        messages: vec![abp_shim_claude::Message {
            role: abp_shim_claude::Role::User,
            content: vec![abp_shim_claude::ContentBlock::Text {
                text: "Hello".into(),
            }],
        }],
        system: Some("Be concise".into()),
        temperature: Some(0.5),
        stop_sequences: None,
        thinking: None,
        stream: None,
    };
    assert_eq!(req.model, "claude-sonnet-4-20250514");
    assert_eq!(req.max_tokens, 4096);
    assert!(req.system.is_some());
}

#[test]
fn gemini_request_builder_chaining() {
    let req = abp_shim_gemini::GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(abp_shim_gemini::Content::user(vec![
            abp_shim_gemini::Part::text("Hello"),
        ]))
        .generation_config(abp_shim_gemini::GenerationConfig {
            temperature: Some(0.8),
            max_output_tokens: Some(2048),
            ..Default::default()
        });
    assert_eq!(req.model, "gemini-2.5-flash");
    assert_eq!(req.contents.len(), 1);
    assert!(req.generation_config.is_some());
}

#[test]
fn codex_request_builder_constructs() {
    let req = abp_shim_codex::CodexRequestBuilder::new()
        .model("codex-mini-latest")
        .input(vec![abp_shim_codex::codex_message("user", "Write code")])
        .max_output_tokens(4096)
        .temperature(0.0)
        .build();
    assert_eq!(req.model, "codex-mini-latest");
    assert_eq!(req.max_output_tokens, Some(4096));
    assert_eq!(req.temperature, Some(0.0));
}

#[test]
fn kimi_request_builder_constructs() {
    let req = abp_shim_kimi::KimiRequestBuilder::new()
        .model("moonshot-v1-128k")
        .messages(vec![
            abp_shim_kimi::Message::system("You are Kimi"),
            abp_shim_kimi::Message::user("Hello"),
        ])
        .max_tokens(2048)
        .temperature(0.3)
        .build();
    assert_eq!(req.model, "moonshot-v1-128k");
    assert_eq!(req.max_tokens, Some(2048));
}

#[test]
fn copilot_request_builder_constructs() {
    let req = abp_shim_copilot::CopilotRequestBuilder::new()
        .model("gpt-4o")
        .messages(vec![
            abp_shim_copilot::Message::system("You are a copilot"),
            abp_shim_copilot::Message::user("Help me"),
        ])
        .build();
    assert_eq!(req.model, "gpt-4o");
    assert_eq!(req.messages.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Shim response parsing
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_response_deserializes() {
    let json = json!({
        "id": "chatcmpl-abc",
        "object": "chat.completion",
        "created": 1700000000u64,
        "model": "gpt-4o",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": "Hello!"
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 5,
            "total_tokens": 15
        }
    });
    let resp: abp_shim_openai::ChatCompletionResponse = serde_json::from_value(json).unwrap();
    assert_eq!(resp.choices.len(), 1);
    assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hello!"));
    assert_eq!(resp.usage.as_ref().unwrap().total_tokens, 15);
}

#[test]
fn claude_response_deserializes() {
    let json = json!({
        "id": "msg_abc",
        "type": "message",
        "role": "assistant",
        "content": [{"type": "text", "text": "Hi there"}],
        "model": "claude-sonnet-4-20250514",
        "stop_reason": "end_turn",
        "stop_sequence": null,
        "usage": {"input_tokens": 10, "output_tokens": 5}
    });
    let resp: abp_shim_claude::MessageResponse = serde_json::from_value(json).unwrap();
    assert_eq!(resp.role, "assistant");
    assert_eq!(resp.content.len(), 1);
}

#[test]
fn gemini_response_with_text() {
    let resp = abp_shim_gemini::GenerateContentResponse {
        candidates: vec![abp_shim_gemini::Candidate {
            content: abp_shim_gemini::Content::model(vec![abp_shim_gemini::Part::text(
                "Hello from Gemini",
            )]),
            finish_reason: Some("STOP".into()),
            safety_ratings: None,
        }],
        usage_metadata: Some(abp_shim_gemini::UsageMetadata {
            prompt_token_count: 5,
            candidates_token_count: 10,
            total_token_count: 15,
        }),
        prompt_feedback: None,
    };
    assert_eq!(resp.text(), Some("Hello from Gemini"));
    assert_eq!(resp.candidates.len(), 1);
}

#[test]
fn codex_response_deserializes() {
    let json = json!({
        "id": "resp_abc",
        "model": "codex-mini-latest",
        "output": [
            {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Done"}]}
        ],
        "usage": {"input_tokens": 10, "output_tokens": 5, "total_tokens": 15},
        "status": "completed"
    });
    let resp: abp_codex_sdk::dialect::CodexResponse = serde_json::from_value(json).unwrap();
    assert_eq!(resp.model, "codex-mini-latest");
}

#[test]
fn kimi_response_deserializes() {
    let json = json!({
        "id": "cmpl-abc",
        "model": "moonshot-v1-8k",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": "Hello from Kimi"},
            "finish_reason": "stop"
        }]
    });
    let resp: abp_kimi_sdk::dialect::KimiResponse = serde_json::from_value(json).unwrap();
    assert_eq!(resp.choices.len(), 1);
}

#[test]
fn copilot_response_deserializes() {
    let json = json!({
        "message": "Hello from Copilot",
        "copilot_references": [],
        "copilot_errors": []
    });
    let resp: abp_copilot_sdk::dialect::CopilotResponse = serde_json::from_value(json).unwrap();
    assert_eq!(resp.message, "Hello from Copilot");
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Shim-to-IR lowering
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_request_to_ir_preserves_messages() {
    let req = abp_shim_openai::ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![
            abp_shim_openai::Message::system("Be helpful"),
            abp_shim_openai::Message::user("Hi"),
        ])
        .build();
    let ir = abp_shim_openai::request_to_ir(&req);
    assert_eq!(ir.len(), 2);
    assert_eq!(ir.messages[0].role, abp_core::ir::IrRole::System);
    assert_eq!(ir.messages[1].role, abp_core::ir::IrRole::User);
}

#[test]
fn openai_messages_to_ir_lowering() {
    let msgs = vec![
        abp_shim_openai::Message::user("hello"),
        abp_shim_openai::Message::assistant("hi there"),
    ];
    let ir = abp_shim_openai::messages_to_ir(&msgs);
    assert_eq!(ir.len(), 2);
    assert_eq!(ir.messages[0].role, abp_core::ir::IrRole::User);
    assert_eq!(ir.messages[1].role, abp_core::ir::IrRole::Assistant);
}

#[test]
fn claude_request_to_work_order_lowering() {
    let req = abp_shim_claude::MessageRequest {
        model: "claude-sonnet-4-20250514".to_string(),
        max_tokens: 1024,
        messages: vec![abp_shim_claude::Message {
            role: abp_shim_claude::Role::User,
            content: vec![abp_shim_claude::ContentBlock::Text {
                text: "Explain Rust".into(),
            }],
        }],
        system: None,
        temperature: None,
        stop_sequences: None,
        thinking: None,
        stream: None,
    };
    let wo = abp_shim_claude::request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("claude-sonnet-4-20250514"));
}

#[test]
fn codex_request_to_ir_preserves_messages() {
    let req = abp_shim_codex::CodexRequestBuilder::new()
        .input(vec![abp_shim_codex::codex_message(
            "user",
            "Write hello world",
        )])
        .build();
    let ir = abp_shim_codex::request_to_ir(&req);
    assert!(!ir.is_empty());
    assert_eq!(ir.messages[0].role, abp_core::ir::IrRole::User);
}

#[test]
fn kimi_request_to_ir_preserves_roles() {
    let req = abp_shim_kimi::KimiRequestBuilder::new()
        .messages(vec![
            abp_shim_kimi::Message::system("system prompt"),
            abp_shim_kimi::Message::user("user msg"),
        ])
        .build();
    let ir = abp_shim_kimi::request_to_ir(&req);
    assert_eq!(ir.len(), 2);
    assert_eq!(ir.messages[0].role, abp_core::ir::IrRole::System);
}

#[test]
fn copilot_request_to_ir_preserves_roles() {
    let req = abp_shim_copilot::CopilotRequestBuilder::new()
        .messages(vec![
            abp_shim_copilot::Message::system("Be concise"),
            abp_shim_copilot::Message::user("Hello"),
        ])
        .build();
    let ir = abp_shim_copilot::request_to_ir(&req);
    assert_eq!(ir.len(), 2);
    assert_eq!(ir.messages[0].role, abp_core::ir::IrRole::System);
    assert_eq!(ir.messages[1].role, abp_core::ir::IrRole::User);
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. IR-to-shim raising
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_ir_to_messages_raising() {
    let ir = abp_core::ir::IrConversation::from_messages(vec![
        abp_core::ir::IrMessage::text(abp_core::ir::IrRole::User, "Hello".to_string()),
        abp_core::ir::IrMessage::text(abp_core::ir::IrRole::Assistant, "Hi".to_string()),
    ]);
    let msgs = abp_shim_openai::ir_to_messages(&ir);
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].role, abp_shim_openai::Role::User);
    assert_eq!(msgs[1].role, abp_shim_openai::Role::Assistant);
}

#[test]
fn kimi_ir_to_messages_raising() {
    let ir = abp_core::ir::IrConversation::from_messages(vec![
        abp_core::ir::IrMessage::text(abp_core::ir::IrRole::System, "sys".to_string()),
        abp_core::ir::IrMessage::text(abp_core::ir::IrRole::User, "hi".to_string()),
    ]);
    let msgs = abp_shim_kimi::ir_to_messages(&ir);
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].role, "system");
    assert_eq!(msgs[1].role, "user");
}

#[test]
fn copilot_ir_to_messages_raising() {
    let ir = abp_core::ir::IrConversation::from_messages(vec![
        abp_core::ir::IrMessage::text(abp_core::ir::IrRole::User, "question".to_string()),
        abp_core::ir::IrMessage::text(abp_core::ir::IrRole::Assistant, "answer".to_string()),
    ]);
    let msgs = abp_shim_copilot::ir_to_messages(&ir);
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].role, "user");
    assert_eq!(msgs[0].content, "question");
    assert_eq!(msgs[1].role, "assistant");
}

#[test]
fn openai_messages_roundtrip_through_ir() {
    let original = vec![
        abp_shim_openai::Message::system("System"),
        abp_shim_openai::Message::user("User msg"),
        abp_shim_openai::Message::assistant("Reply"),
    ];
    let ir = abp_shim_openai::messages_to_ir(&original);
    let back = abp_shim_openai::ir_to_messages(&ir);
    assert_eq!(back.len(), 3);
    assert_eq!(back[0].role, abp_shim_openai::Role::System);
    assert_eq!(back[1].role, abp_shim_openai::Role::User);
    assert_eq!(back[2].role, abp_shim_openai::Role::Assistant);
}

#[test]
fn kimi_messages_roundtrip_through_ir() {
    let original = vec![
        abp_shim_kimi::Message::system("sys"),
        abp_shim_kimi::Message::user("user"),
        abp_shim_kimi::Message::assistant("reply"),
    ];
    let ir = abp_shim_kimi::messages_to_ir(&original);
    let back = abp_shim_kimi::ir_to_messages(&ir);
    assert_eq!(back.len(), 3);
    assert_eq!(back[0].role, "system");
    assert_eq!(back[2].role, "assistant");
}

#[test]
fn copilot_messages_roundtrip_through_ir() {
    let original = vec![
        abp_shim_copilot::Message::system("sys"),
        abp_shim_copilot::Message::user("question"),
        abp_shim_copilot::Message::assistant("answer"),
    ];
    let ir = abp_shim_copilot::messages_to_ir(&original);
    let back = abp_shim_copilot::ir_to_messages(&ir);
    assert_eq!(back.len(), 3);
    assert_eq!(back[0].role, "system");
    assert_eq!(back[1].content, "question");
}

#[test]
fn codex_response_to_ir_and_back() {
    let ir = abp_core::ir::IrConversation::from_messages(vec![abp_core::ir::IrMessage::text(
        abp_core::ir::IrRole::Assistant,
        "Done".to_string(),
    )]);
    let items = abp_shim_codex::ir_to_response_items(&ir);
    assert!(!items.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Shim streaming types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_stream_event_serde() {
    let evt = abp_shim_openai::StreamEvent {
        id: "chatcmpl-1".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "gpt-4o".into(),
        choices: vec![abp_shim_openai::StreamChoice {
            index: 0,
            delta: abp_shim_openai::Delta {
                role: Some("assistant".into()),
                content: Some("Hi".into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
    };
    let json = serde_json::to_string(&evt).unwrap();
    let back: abp_shim_openai::StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(back.choices[0].delta.content.as_deref(), Some("Hi"));
}

#[test]
fn claude_stream_event_variants_serde() {
    let events = vec![
        abp_shim_claude::StreamEvent::Ping {},
        abp_shim_claude::StreamEvent::MessageStop {},
        abp_shim_claude::StreamEvent::ContentBlockStop { index: 0 },
    ];
    for evt in &events {
        let json = serde_json::to_string(evt).unwrap();
        let _back: abp_shim_claude::StreamEvent = serde_json::from_str(&json).unwrap();
    }
}

#[test]
fn claude_stream_delta_text_serde() {
    let delta = abp_shim_claude::StreamDelta::TextDelta {
        text: "hello".into(),
    };
    let json = serde_json::to_string(&delta).unwrap();
    let back: abp_shim_claude::StreamDelta = serde_json::from_str(&json).unwrap();
    assert_eq!(back, delta);
}

#[test]
fn gemini_stream_event_with_text() {
    let evt = abp_shim_gemini::StreamEvent {
        candidates: vec![abp_shim_gemini::Candidate {
            content: abp_shim_gemini::Content::model(vec![abp_shim_gemini::Part::text("chunk")]),
            finish_reason: None,
            safety_ratings: None,
        }],
        usage_metadata: None,
    };
    assert_eq!(evt.text(), Some("chunk"));
}

#[test]
fn openai_delta_default_is_empty() {
    let delta = abp_shim_openai::Delta::default();
    assert!(delta.role.is_none());
    assert!(delta.content.is_none());
    assert!(delta.tool_calls.is_none());
}

#[test]
fn openai_stream_tool_call_serde() {
    let stc = abp_shim_openai::StreamToolCall {
        index: 0,
        id: Some("call_1".into()),
        call_type: Some("function".into()),
        function: Some(abp_shim_openai::StreamFunctionCall {
            name: Some("read_file".into()),
            arguments: Some(r#"{"path":"main.rs"}"#.into()),
        }),
    };
    let json = serde_json::to_string(&stc).unwrap();
    let back: abp_shim_openai::StreamToolCall = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id.as_deref(), Some("call_1"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Shim error types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_shim_error_display() {
    let err = abp_shim_openai::ShimError::InvalidRequest("bad input".into());
    assert!(err.to_string().contains("bad input"));
}

#[test]
fn openai_shim_error_internal() {
    let err = abp_shim_openai::ShimError::Internal("something broke".into());
    assert!(err.to_string().contains("something broke"));
}

#[test]
fn openai_shim_error_serde() {
    let serde_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
    let err = abp_shim_openai::ShimError::from(serde_err);
    assert!(err.to_string().contains("serde"));
}

#[test]
fn claude_shim_error_display() {
    let err = abp_shim_claude::ShimError::InvalidRequest("empty messages".into());
    assert!(err.to_string().contains("empty messages"));
}

#[test]
fn claude_shim_error_api_error() {
    let err = abp_shim_claude::ShimError::ApiError {
        error_type: "rate_limit".into(),
        message: "too many requests".into(),
    };
    assert!(err.to_string().contains("rate_limit"));
    assert!(err.to_string().contains("too many requests"));
}

#[test]
fn gemini_error_display() {
    let err = abp_shim_gemini::GeminiError::RequestConversion("bad model".into());
    assert!(err.to_string().contains("bad model"));
}

#[test]
fn gemini_error_backend() {
    let err = abp_shim_gemini::GeminiError::BackendError("timeout".into());
    assert!(err.to_string().contains("timeout"));
}

#[test]
fn codex_shim_error_display() {
    let err = abp_shim_codex::ShimError::InvalidRequest("missing input".into());
    assert!(err.to_string().contains("missing input"));
}

#[test]
fn kimi_shim_error_display() {
    let err = abp_shim_kimi::ShimError::Internal("failed".into());
    assert!(err.to_string().contains("failed"));
}

#[test]
fn copilot_shim_error_display() {
    let err = abp_shim_copilot::ShimError::InvalidRequest("no messages".into());
    assert!(err.to_string().contains("no messages"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Cross-shim type compatibility
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_and_kimi_usage_fields_match_semantics() {
    // Both OpenAI and Kimi use prompt_tokens/completion_tokens/total_tokens
    let openai_usage = abp_shim_openai::Usage {
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
    };
    let kimi_usage = abp_shim_kimi::Usage {
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
    };
    assert_eq!(openai_usage.prompt_tokens, kimi_usage.prompt_tokens);
    assert_eq!(openai_usage.completion_tokens, kimi_usage.completion_tokens);
    assert_eq!(openai_usage.total_tokens, kimi_usage.total_tokens);
}

#[test]
fn codex_and_gemini_usage_fields_map_correctly() {
    let codex_usage = abp_shim_codex::Usage {
        input_tokens: 100,
        output_tokens: 50,
        total_tokens: 150,
    };
    let gemini_usage = abp_shim_gemini::UsageMetadata {
        prompt_token_count: 100,
        candidates_token_count: 50,
        total_token_count: 150,
    };
    // Input/prompt and output/candidates should have equivalent semantics
    assert_eq!(codex_usage.input_tokens, gemini_usage.prompt_token_count);
    assert_eq!(
        codex_usage.output_tokens,
        gemini_usage.candidates_token_count
    );
    assert_eq!(codex_usage.total_tokens, gemini_usage.total_token_count);
}

#[test]
fn all_shims_lower_user_message_to_ir_user_role() {
    // OpenAI
    let openai_ir = abp_shim_openai::messages_to_ir(&[abp_shim_openai::Message::user("test")]);
    assert_eq!(openai_ir.messages[0].role, abp_core::ir::IrRole::User);

    // Kimi
    let kimi_ir = abp_shim_kimi::messages_to_ir(&[abp_shim_kimi::Message::user("test")]);
    assert_eq!(kimi_ir.messages[0].role, abp_core::ir::IrRole::User);

    // Copilot
    let copilot_ir = abp_shim_copilot::messages_to_ir(&[abp_shim_copilot::Message::user("test")]);
    assert_eq!(copilot_ir.messages[0].role, abp_core::ir::IrRole::User);
}

#[test]
fn all_shims_lower_system_message_to_ir_system_role() {
    let openai_ir = abp_shim_openai::messages_to_ir(&[abp_shim_openai::Message::system("prompt")]);
    assert_eq!(openai_ir.messages[0].role, abp_core::ir::IrRole::System);

    let kimi_ir = abp_shim_kimi::messages_to_ir(&[abp_shim_kimi::Message::system("prompt")]);
    assert_eq!(kimi_ir.messages[0].role, abp_core::ir::IrRole::System);

    let copilot_ir =
        abp_shim_copilot::messages_to_ir(&[abp_shim_copilot::Message::system("prompt")]);
    assert_eq!(copilot_ir.messages[0].role, abp_core::ir::IrRole::System);
}

#[test]
fn all_shims_lower_assistant_message_to_ir_assistant_role() {
    let openai_ir = abp_shim_openai::messages_to_ir(&[abp_shim_openai::Message::assistant("ok")]);
    assert_eq!(openai_ir.messages[0].role, abp_core::ir::IrRole::Assistant);

    let kimi_ir = abp_shim_kimi::messages_to_ir(&[abp_shim_kimi::Message::assistant("ok")]);
    assert_eq!(kimi_ir.messages[0].role, abp_core::ir::IrRole::Assistant);

    let copilot_ir =
        abp_shim_copilot::messages_to_ir(&[abp_shim_copilot::Message::assistant("ok")]);
    assert_eq!(copilot_ir.messages[0].role, abp_core::ir::IrRole::Assistant);
}

#[test]
fn cross_shim_ir_text_content_preserved() {
    let text = "The quick brown fox jumps over the lazy dog";

    let openai_ir = abp_shim_openai::messages_to_ir(&[abp_shim_openai::Message::user(text)]);
    let kimi_ir = abp_shim_kimi::messages_to_ir(&[abp_shim_kimi::Message::user(text)]);
    let copilot_ir = abp_shim_copilot::messages_to_ir(&[abp_shim_copilot::Message::user(text)]);

    assert_eq!(openai_ir.messages[0].text_content(), text);
    assert_eq!(kimi_ir.messages[0].text_content(), text);
    assert_eq!(copilot_ir.messages[0].text_content(), text);
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Shim API surface completeness
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_client_exposes_chat_completions() {
    let client = abp_shim_openai::OpenAiClient::new("gpt-4o");
    let _chat = client.chat();
    let _completions = client.chat().completions();
}

#[test]
fn openai_message_has_all_constructors() {
    let _ = abp_shim_openai::Message::system("sys");
    let _ = abp_shim_openai::Message::user("usr");
    let _ = abp_shim_openai::Message::assistant("asst");
    let _ = abp_shim_openai::Message::tool("call_1", "result");
    let _ = abp_shim_openai::Message::assistant_with_tool_calls(vec![]);
}

#[test]
fn claude_content_block_has_all_variants() {
    let _ = abp_shim_claude::ContentBlock::Text { text: "t".into() };
    let _ = abp_shim_claude::ContentBlock::ToolUse {
        id: "id".into(),
        name: "n".into(),
        input: json!({}),
    };
    let _ = abp_shim_claude::ContentBlock::ToolResult {
        tool_use_id: "id".into(),
        content: Some("ok".into()),
        is_error: Some(false),
    };
    let _ = abp_shim_claude::ContentBlock::Thinking {
        thinking: "hmm".into(),
        signature: None,
    };
    let _ = abp_shim_claude::ContentBlock::Image {
        source: abp_shim_claude::ImageSource::Url {
            url: "https://example.com/img.png".into(),
        },
    };
}

#[test]
fn gemini_part_has_all_constructors() {
    let _ = abp_shim_gemini::Part::text("hi");
    let _ = abp_shim_gemini::Part::inline_data("image/png", "base64data");
    let _ = abp_shim_gemini::Part::function_call("fn", json!({}));
    let _ = abp_shim_gemini::Part::function_response("fn", json!({"result": "ok"}));
}

#[test]
fn gemini_content_has_role_constructors() {
    let user = abp_shim_gemini::Content::user(vec![abp_shim_gemini::Part::text("hi")]);
    let model = abp_shim_gemini::Content::model(vec![abp_shim_gemini::Part::text("hi")]);
    assert_eq!(user.role, "user");
    assert_eq!(model.role, "model");
}

#[test]
fn kimi_message_has_all_constructors() {
    let _ = abp_shim_kimi::Message::system("sys");
    let _ = abp_shim_kimi::Message::user("usr");
    let _ = abp_shim_kimi::Message::assistant("asst");
    let _ = abp_shim_kimi::Message::tool("call_1", "result");
    let _ = abp_shim_kimi::Message::assistant_with_tool_calls(vec![]);
}

#[test]
fn copilot_message_has_all_constructors() {
    let _ = abp_shim_copilot::Message::system("sys");
    let _ = abp_shim_copilot::Message::user("usr");
    let _ = abp_shim_copilot::Message::assistant("asst");
    let _ = abp_shim_copilot::Message::user_with_refs("usr", vec![]);
}

#[test]
fn openai_role_has_all_variants() {
    let roles = [
        abp_shim_openai::Role::System,
        abp_shim_openai::Role::User,
        abp_shim_openai::Role::Assistant,
        abp_shim_openai::Role::Tool,
    ];
    assert_eq!(roles.len(), 4);
}

#[test]
fn claude_role_has_user_and_assistant() {
    let roles = [
        abp_shim_claude::Role::User,
        abp_shim_claude::Role::Assistant,
    ];
    assert_eq!(roles.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. Shim model name handling
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_builder_defaults_model_to_gpt4o() {
    let req = abp_shim_openai::ChatCompletionRequest::builder()
        .messages(vec![abp_shim_openai::Message::user("test")])
        .build();
    assert_eq!(req.model, "gpt-4o");
}

#[test]
fn openai_model_preserved_in_work_order() {
    let req = abp_shim_openai::ChatCompletionRequest::builder()
        .model("gpt-4-turbo")
        .messages(vec![abp_shim_openai::Message::user("test")])
        .build();
    let wo = abp_shim_openai::request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4-turbo"));
}

#[test]
fn codex_builder_defaults_model_to_codex_mini() {
    let req = abp_shim_codex::CodexRequestBuilder::new()
        .input(vec![abp_shim_codex::codex_message("user", "hi")])
        .build();
    assert_eq!(req.model, "codex-mini-latest");
}

#[test]
fn codex_model_preserved_in_work_order() {
    let req = abp_shim_codex::CodexRequestBuilder::new()
        .model("o3-pro")
        .input(vec![abp_shim_codex::codex_message("user", "hi")])
        .build();
    let wo = abp_shim_codex::request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("o3-pro"));
}

#[test]
fn kimi_builder_defaults_model_to_moonshot() {
    let req = abp_shim_kimi::KimiRequestBuilder::new()
        .messages(vec![abp_shim_kimi::Message::user("test")])
        .build();
    assert_eq!(req.model, "moonshot-v1-8k");
}

#[test]
fn kimi_model_preserved_in_work_order() {
    let req = abp_shim_kimi::KimiRequestBuilder::new()
        .model("moonshot-v1-128k")
        .messages(vec![abp_shim_kimi::Message::user("test")])
        .build();
    let wo = abp_shim_kimi::request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-128k"));
}

#[test]
fn copilot_builder_defaults_model_to_gpt4o() {
    let req = abp_shim_copilot::CopilotRequestBuilder::new()
        .messages(vec![abp_shim_copilot::Message::user("test")])
        .build();
    assert_eq!(req.model, "gpt-4o");
}

#[test]
fn copilot_model_preserved_in_work_order() {
    let req = abp_shim_copilot::CopilotRequestBuilder::new()
        .model("o3-mini")
        .messages(vec![abp_shim_copilot::Message::user("test")])
        .build();
    let wo = abp_shim_copilot::request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("o3-mini"));
}

#[test]
fn gemini_client_model_accessor() {
    let client = abp_shim_gemini::GeminiClient::new("gemini-2.5-pro");
    assert_eq!(client.model(), "gemini-2.5-pro");
}

#[test]
fn openai_client_model_accessor() {
    let client = abp_shim_openai::OpenAiClient::new("gpt-4o-mini");
    assert_eq!(client.model(), "gpt-4o-mini");
}

#[test]
fn kimi_client_model_accessor() {
    let client = abp_shim_kimi::KimiClient::new("moonshot-v1-32k");
    assert_eq!(client.model(), "moonshot-v1-32k");
}

#[test]
fn copilot_client_model_accessor() {
    let client = abp_shim_copilot::CopilotClient::new("gpt-4-turbo");
    assert_eq!(client.model(), "gpt-4-turbo");
}

#[test]
fn codex_client_model_accessor() {
    let client = abp_shim_codex::CodexClient::new("codex-mini-latest");
    assert_eq!(client.model(), "codex-mini-latest");
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. Shim capability mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_ir_usage_conversion() {
    let ir = abp_core::ir::IrUsage::from_io(100, 50);
    let usage = abp_shim_openai::ir_usage_to_usage(&ir);
    assert_eq!(usage.prompt_tokens, 100);
    assert_eq!(usage.completion_tokens, 50);
    assert_eq!(usage.total_tokens, 150);
}

#[test]
fn codex_ir_usage_conversion() {
    let ir = abp_core::ir::IrUsage::from_io(200, 100);
    let usage = abp_shim_codex::ir_usage_to_usage(&ir);
    assert_eq!(usage.input_tokens, 200);
    assert_eq!(usage.output_tokens, 100);
    assert_eq!(usage.total_tokens, 300);
}

#[test]
fn kimi_ir_usage_conversion() {
    let ir = abp_core::ir::IrUsage::from_io(300, 150);
    let usage = abp_shim_kimi::ir_usage_to_usage(&ir);
    assert_eq!(usage.prompt_tokens, 300);
    assert_eq!(usage.completion_tokens, 150);
    assert_eq!(usage.total_tokens, 450);
}

#[test]
fn copilot_ir_usage_tuple_conversion() {
    let ir = abp_core::ir::IrUsage::from_io(500, 250);
    let (input, output, total) = abp_shim_copilot::ir_usage_to_tuple(&ir);
    assert_eq!(input, 500);
    assert_eq!(output, 250);
    assert_eq!(total, 750);
}

#[test]
fn gemini_usage_to_ir_conversion() {
    let usage = abp_shim_gemini::UsageMetadata {
        prompt_token_count: 80,
        candidates_token_count: 40,
        total_token_count: 120,
    };
    let ir = abp_shim_gemini::usage_to_ir(&usage);
    assert_eq!(ir.input_tokens, 80);
    assert_eq!(ir.output_tokens, 40);
    assert_eq!(ir.total_tokens, 120);
}

#[test]
fn gemini_usage_from_ir_conversion() {
    let ir = abp_core::ir::IrUsage::from_io(60, 30);
    let usage = abp_shim_gemini::usage_from_ir(&ir);
    assert_eq!(usage.prompt_token_count, 60);
    assert_eq!(usage.candidates_token_count, 30);
    assert_eq!(usage.total_token_count, 90);
}

#[test]
fn openai_tools_to_ir_conversion() {
    let tools = vec![abp_shim_openai::Tool::function(
        "my_tool",
        "Does something",
        json!({"type": "object"}),
    )];
    let ir_tools = abp_shim_openai::tools_to_ir(&tools);
    assert_eq!(ir_tools.len(), 1);
    assert_eq!(ir_tools[0].name, "my_tool");
    assert_eq!(ir_tools[0].description, "Does something");
}

#[test]
fn claude_content_block_ir_roundtrip() {
    let block = abp_shim_claude::ContentBlock::ToolUse {
        id: "tu_1".into(),
        name: "bash".into(),
        input: json!({"command": "ls -la"}),
    };
    let ir_block = abp_shim_claude::content_block_to_ir(&block);
    let back = abp_shim_claude::content_block_from_ir(&ir_block);
    assert_eq!(back, block);
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. Default construction for each shim type
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_builder_default() {
    let builder = abp_shim_openai::ChatCompletionRequest::builder();
    let req = builder.build();
    assert_eq!(req.model, "gpt-4o");
    assert!(req.messages.is_empty());
    assert!(req.tools.is_none());
    assert!(req.temperature.is_none());
}

#[test]
fn codex_builder_default() {
    let req = abp_shim_codex::CodexRequestBuilder::new().build();
    assert_eq!(req.model, "codex-mini-latest");
    assert!(req.input.is_empty());
    assert!(req.max_output_tokens.is_none());
}

#[test]
fn kimi_builder_default() {
    let req = abp_shim_kimi::KimiRequestBuilder::new().build();
    assert_eq!(req.model, "moonshot-v1-8k");
    assert!(req.max_tokens.is_none());
    assert!(req.temperature.is_none());
}

#[test]
fn copilot_builder_default() {
    let req = abp_shim_copilot::CopilotRequestBuilder::new().build();
    assert_eq!(req.model, "gpt-4o");
    assert!(req.messages.is_empty());
}

#[test]
fn anthropic_client_default() {
    let client = abp_shim_claude::AnthropicClient::new();
    let debug = format!("{client:?}");
    assert!(debug.contains("AnthropicClient"));
}

#[test]
fn openai_delta_default() {
    let delta = abp_shim_openai::Delta::default();
    assert!(delta.role.is_none());
    assert!(delta.content.is_none());
    assert!(delta.tool_calls.is_none());
}

#[test]
fn gemini_generation_config_default() {
    let cfg = abp_shim_gemini::GenerationConfig::default();
    assert!(cfg.temperature.is_none());
    assert!(cfg.max_output_tokens.is_none());
    assert!(cfg.top_p.is_none());
    assert!(cfg.top_k.is_none());
    assert!(cfg.stop_sequences.is_none());
}

#[test]
fn gemini_request_new_minimal() {
    let req = abp_shim_gemini::GenerateContentRequest::new("gemini-2.5-flash");
    assert_eq!(req.model, "gemini-2.5-flash");
    assert!(req.contents.is_empty());
    assert!(req.system_instruction.is_none());
    assert!(req.generation_config.is_none());
    assert!(req.tools.is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// Additional coverage: work order construction, receipts, multi-message
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_request_to_work_order_extracts_task() {
    let req = abp_shim_openai::ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![
            abp_shim_openai::Message::system("You are helpful"),
            abp_shim_openai::Message::user("Explain Rust ownership"),
        ])
        .build();
    let wo = abp_shim_openai::request_to_work_order(&req);
    assert!(wo.task.contains("Rust"));
}

#[test]
fn codex_request_to_work_order_extracts_task() {
    let req = abp_shim_codex::CodexRequestBuilder::new()
        .input(vec![abp_shim_codex::codex_message(
            "user",
            "Fix the bug in main.rs",
        )])
        .build();
    let wo = abp_shim_codex::request_to_work_order(&req);
    assert!(wo.task.contains("bug") || wo.task.contains("main"));
}

#[test]
fn kimi_request_to_work_order_extracts_task() {
    let req = abp_shim_kimi::KimiRequestBuilder::new()
        .messages(vec![abp_shim_kimi::Message::user("Translate to French")])
        .build();
    let wo = abp_shim_kimi::request_to_work_order(&req);
    assert!(wo.task.contains("Translate") || wo.task.contains("French"));
}

#[test]
fn copilot_request_to_work_order_extracts_task() {
    let req = abp_shim_copilot::CopilotRequestBuilder::new()
        .messages(vec![abp_shim_copilot::Message::user("Generate unit tests")])
        .build();
    let wo = abp_shim_copilot::request_to_work_order(&req);
    assert!(wo.task.contains("unit tests") || wo.task.contains("Generate"));
}

#[test]
fn openai_tool_call_serde_roundtrip() {
    let tc = abp_shim_openai::ToolCall {
        id: "call_abc".into(),
        call_type: "function".into(),
        function: abp_shim_openai::FunctionCall {
            name: "read_file".into(),
            arguments: r#"{"path":"main.rs"}"#.into(),
        },
    };
    let json = serde_json::to_string(&tc).unwrap();
    let back: abp_shim_openai::ToolCall = serde_json::from_str(&json).unwrap();
    assert_eq!(back, tc);
}

#[test]
fn claude_api_error_serde_roundtrip() {
    let err = abp_shim_claude::ApiError {
        error_type: "invalid_request_error".into(),
        message: "Missing required field".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: abp_shim_claude::ApiError = serde_json::from_str(&json).unwrap();
    assert_eq!(back, err);
}

#[test]
fn claude_message_delta_payload_serde() {
    let payload = abp_shim_claude::MessageDeltaPayload {
        stop_reason: Some("end_turn".into()),
        stop_sequence: None,
    };
    let json = serde_json::to_string(&payload).unwrap();
    let back: abp_shim_claude::MessageDeltaPayload = serde_json::from_str(&json).unwrap();
    assert_eq!(back, payload);
}

#[test]
fn gemini_function_call_part_serde() {
    let part = abp_shim_gemini::Part::function_call("get_weather", json!({"location": "London"}));
    let json = serde_json::to_string(&part).unwrap();
    let back: abp_shim_gemini::Part = serde_json::from_str(&json).unwrap();
    assert_eq!(back, part);
}

#[test]
fn gemini_safety_setting_serde() {
    let setting = abp_shim_gemini::SafetySetting {
        category: abp_shim_gemini::HarmCategory::HarmCategoryDangerousContent,
        threshold: abp_shim_gemini::HarmBlockThreshold::BlockMediumAndAbove,
    };
    let json = serde_json::to_string(&setting).unwrap();
    let back: abp_shim_gemini::SafetySetting = serde_json::from_str(&json).unwrap();
    assert_eq!(back, setting);
}

#[test]
fn all_shims_produce_debug_output() {
    // Verify Debug is implemented for all client types
    let openai = abp_shim_openai::OpenAiClient::new("gpt-4o");
    let claude = abp_shim_claude::AnthropicClient::new();
    let gemini = abp_shim_gemini::GeminiClient::new("gemini-2.5-flash");
    let codex = abp_shim_codex::CodexClient::new("codex-mini-latest");
    let kimi = abp_shim_kimi::KimiClient::new("moonshot-v1-8k");
    let copilot = abp_shim_copilot::CopilotClient::new("gpt-4o");

    assert!(!format!("{openai:?}").is_empty());
    assert!(!format!("{claude:?}").is_empty());
    assert!(!format!("{gemini:?}").is_empty());
    assert!(!format!("{codex:?}").is_empty());
    assert!(!format!("{kimi:?}").is_empty());
    assert!(!format!("{copilot:?}").is_empty());
}

#[test]
fn openai_re_exports_accessible() {
    // Verify re-exported types from the SDK are accessible
    let _mode = abp_shim_openai::ToolChoiceMode::Auto;
    let _: Option<abp_shim_openai::ResponseFormat> = None;
}

#[test]
fn gemini_re_exports_accessible() {
    let _mode = abp_shim_gemini::FunctionCallingMode::Auto;
    let _cat = abp_shim_gemini::HarmCategory::HarmCategoryHarassment;
    let _thresh = abp_shim_gemini::HarmBlockThreshold::BlockOnlyHigh;
}

#[test]
fn codex_re_exports_accessible() {
    let _: Option<abp_shim_codex::CodexShimRequest> = None;
    let _: Option<abp_shim_codex::CodexFunctionDef> = None;
}

#[test]
fn kimi_re_exports_accessible() {
    let _role = abp_shim_kimi::KimiRole::User;
    let _: Option<abp_shim_kimi::KimiFunctionDef> = None;
    let _: Option<abp_shim_kimi::KimiToolDef> = None;
}

#[test]
fn copilot_re_exports_accessible() {
    let _: Option<abp_shim_copilot::CopilotFunctionDef> = None;
    let _tt = abp_shim_copilot::CopilotToolType::Function;
}

#[test]
fn gemini_tool_declaration_serde() {
    let td = abp_shim_gemini::ToolDeclaration {
        function_declarations: vec![abp_shim_gemini::FunctionDeclaration {
            name: "search".into(),
            description: "Search the web".into(),
            parameters: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
        }],
    };
    let json = serde_json::to_string(&td).unwrap();
    let back: abp_shim_gemini::ToolDeclaration = serde_json::from_str(&json).unwrap();
    assert_eq!(back, td);
}

#[test]
fn gemini_tool_config_serde() {
    let tc = abp_shim_gemini::ToolConfig {
        function_calling_config: abp_shim_gemini::FunctionCallingConfig {
            mode: abp_shim_gemini::FunctionCallingMode::Auto,
            allowed_function_names: Some(vec!["search".into()]),
        },
    };
    let json = serde_json::to_string(&tc).unwrap();
    let back: abp_shim_gemini::ToolConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back, tc);
}

#[test]
fn claude_image_source_base64_serde() {
    let source = abp_shim_claude::ImageSource::Base64 {
        media_type: "image/png".into(),
        data: "iVBORw0KGgo=".into(),
    };
    let json = serde_json::to_string(&source).unwrap();
    let back: abp_shim_claude::ImageSource = serde_json::from_str(&json).unwrap();
    assert_eq!(back, source);
}

#[test]
fn claude_image_source_url_serde() {
    let source = abp_shim_claude::ImageSource::Url {
        url: "https://example.com/img.png".into(),
    };
    let json = serde_json::to_string(&source).unwrap();
    let back: abp_shim_claude::ImageSource = serde_json::from_str(&json).unwrap();
    assert_eq!(back, source);
}

#[test]
fn claude_thinking_block_roundtrip() {
    let block = abp_shim_claude::ContentBlock::Thinking {
        thinking: "Let me consider...".into(),
        signature: Some("sig_abc".into()),
    };
    let ir = abp_shim_claude::content_block_to_ir(&block);
    let back = abp_shim_claude::content_block_from_ir(&ir);
    assert_eq!(back, block);
}

#[test]
fn gemini_inline_data_part_serde() {
    let part = abp_shim_gemini::Part::inline_data("image/jpeg", "base64encoded");
    let json = serde_json::to_string(&part).unwrap();
    let back: abp_shim_gemini::Part = serde_json::from_str(&json).unwrap();
    assert_eq!(back, part);
}

#[test]
fn gemini_function_response_part_serde() {
    let part = abp_shim_gemini::Part::function_response("get_weather", json!({"temp": 72}));
    let json = serde_json::to_string(&part).unwrap();
    let back: abp_shim_gemini::Part = serde_json::from_str(&json).unwrap();
    assert_eq!(back, part);
}

#[test]
fn claude_tool_result_block_serde() {
    let block = abp_shim_claude::ContentBlock::ToolResult {
        tool_use_id: "tu_1".into(),
        content: Some("file contents here".into()),
        is_error: Some(false),
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: abp_shim_claude::ContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(back, block);
}

#[test]
fn openai_request_with_stop_sequences() {
    let req = abp_shim_openai::ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![abp_shim_openai::Message::user("test")])
        .stop(vec!["END".into(), "STOP".into()])
        .build();
    assert_eq!(req.stop.as_ref().unwrap().len(), 2);
}

#[test]
fn openai_request_with_stream_flag() {
    let req = abp_shim_openai::ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![abp_shim_openai::Message::user("test")])
        .stream(true)
        .build();
    assert_eq!(req.stream, Some(true));
}

#[test]
fn kimi_request_with_use_search() {
    let req = abp_shim_kimi::KimiRequestBuilder::new()
        .messages(vec![abp_shim_kimi::Message::user("search something")])
        .use_search(true)
        .stream(true)
        .build();
    assert_eq!(req.use_search, Some(true));
    assert_eq!(req.stream, Some(true));
}

#[test]
fn gemini_gen_config_from_dialect_roundtrip() {
    let cfg = abp_shim_gemini::GenerationConfig {
        temperature: Some(0.5),
        max_output_tokens: Some(1024),
        top_p: Some(0.9),
        top_k: Some(40),
        candidate_count: None,
        stop_sequences: Some(vec!["END".into()]),
        response_mime_type: Some("application/json".into()),
        response_schema: Some(json!({"type": "object"})),
    };
    // Verify all fields are preserved after conversion through dialect
    let dialect = abp_gemini_sdk::dialect::GeminiGenerationConfig {
        temperature: cfg.temperature,
        max_output_tokens: cfg.max_output_tokens,
        top_p: cfg.top_p,
        top_k: cfg.top_k,
        candidate_count: None,
        stop_sequences: cfg.stop_sequences.clone(),
        response_mime_type: cfg.response_mime_type.clone(),
        response_schema: cfg.response_schema.clone(),
    };
    let back = abp_shim_gemini::gen_config_from_dialect(&dialect);
    assert_eq!(back.temperature, cfg.temperature);
    assert_eq!(back.max_output_tokens, cfg.max_output_tokens);
    assert_eq!(back.top_p, cfg.top_p);
    assert_eq!(back.top_k, cfg.top_k);
    assert_eq!(back.stop_sequences, cfg.stop_sequences);
}

#[test]
fn gemini_response_function_calls_extraction() {
    let resp = abp_shim_gemini::GenerateContentResponse {
        candidates: vec![abp_shim_gemini::Candidate {
            content: abp_shim_gemini::Content::model(vec![
                abp_shim_gemini::Part::function_call("search", json!({"q": "rust"})),
                abp_shim_gemini::Part::function_call("read", json!({"path": "main.rs"})),
            ]),
            finish_reason: Some("STOP".into()),
            safety_ratings: None,
        }],
        usage_metadata: None,
        prompt_feedback: None,
    };
    let calls = resp.function_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].0, "search");
    assert_eq!(calls[1].0, "read");
}
