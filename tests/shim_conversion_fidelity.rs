// SPDX-License-Identifier: MIT OR Apache-2.0
//! Shim conversion fidelity tests.
//!
//! Validates that each SDK shim's conversion functions faithfully preserve
//! message content, roles, tool calls, usage, and streaming semantics when
//! converting between shim types, IR, WorkOrders, and Receipts.

use abp_core::ir::{IrRole, IrUsage};
use abp_core::{AgentEvent, AgentEventKind};
use chrono::Utc;
use serde_json::json;

// =========================================================================
// Helpers
// =========================================================================

fn agent_event_assistant_msg(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: text.into() },
        ext: None,
    }
}

fn agent_event_assistant_delta(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: text.into() },
        ext: None,
    }
}

fn agent_event_tool_call(name: &str, id: &str, input: serde_json::Value) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: name.into(),
            tool_use_id: Some(id.into()),
            parent_tool_use_id: None,
            input,
        },
        ext: None,
    }
}

fn agent_event_error(msg: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: msg.into(),
            error_code: None,
        },
        ext: None,
    }
}

// =========================================================================
// 1. OpenAI shim: request_to_ir preserves roles and content
// =========================================================================

#[test]
fn openai_request_to_ir_preserves_roles() {
    let req = abp_shim_openai::ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![
            abp_shim_openai::Message::system("Be helpful."),
            abp_shim_openai::Message::user("Hello"),
            abp_shim_openai::Message::assistant("Hi there!"),
        ])
        .build();

    let conv = abp_shim_openai::request_to_ir(&req);
    assert_eq!(conv.len(), 3);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[0].text_content(), "Be helpful.");
    assert_eq!(conv.messages[1].role, IrRole::User);
    assert_eq!(conv.messages[1].text_content(), "Hello");
    assert_eq!(conv.messages[2].role, IrRole::Assistant);
    assert_eq!(conv.messages[2].text_content(), "Hi there!");
}

// =========================================================================
// 2. OpenAI shim: messages_to_ir roundtrip
// =========================================================================

#[test]
fn openai_messages_to_ir_and_back() {
    let msgs = vec![
        abp_shim_openai::Message::system("System"),
        abp_shim_openai::Message::user("User msg"),
        abp_shim_openai::Message::assistant("Reply"),
    ];
    let conv = abp_shim_openai::messages_to_ir(&msgs);
    let back = abp_shim_openai::ir_to_messages(&conv);
    assert_eq!(back.len(), 3);
    assert_eq!(back[0].role, abp_shim_openai::Role::System);
    assert_eq!(back[0].content.as_deref(), Some("System"));
    assert_eq!(back[1].role, abp_shim_openai::Role::User);
    assert_eq!(back[2].role, abp_shim_openai::Role::Assistant);
}

// =========================================================================
// 3. OpenAI shim: request_to_work_order preserves model
// =========================================================================

#[test]
fn openai_request_to_work_order_preserves_model() {
    let req = abp_shim_openai::ChatCompletionRequest::builder()
        .model("gpt-4-turbo")
        .messages(vec![abp_shim_openai::Message::user("test")])
        .temperature(0.5)
        .max_tokens(512)
        .build();

    let wo = abp_shim_openai::request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4-turbo"));
}

// =========================================================================
// 4. OpenAI shim: receipt_to_response assistant message
// =========================================================================

#[test]
fn openai_receipt_to_response_preserves_text() {
    let events = vec![agent_event_assistant_msg("Hello from OpenAI!")];
    let receipt = abp_shim_openai::mock_receipt(events);
    let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");

    assert_eq!(resp.model, "gpt-4o");
    assert!(!resp.choices.is_empty());
    assert_eq!(
        resp.choices[0].message.content.as_deref(),
        Some("Hello from OpenAI!")
    );
}

// =========================================================================
// 5. OpenAI shim: receipt_to_response tool call
// =========================================================================

#[test]
fn openai_receipt_to_response_preserves_tool_call() {
    let events = vec![agent_event_tool_call(
        "read_file",
        "call_123",
        json!({"path": "src/main.rs"}),
    )];
    let receipt = abp_shim_openai::mock_receipt(events);
    let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");

    let tc = resp.choices[0].message.tool_calls.as_ref().unwrap();
    assert_eq!(tc.len(), 1);
    assert_eq!(tc[0].function.name, "read_file");
    assert!(tc[0].function.arguments.contains("main.rs"));
}

// =========================================================================
// 6. OpenAI shim: ir_usage_to_usage
// =========================================================================

#[test]
fn openai_ir_usage_to_usage_maps_fields() {
    let ir = IrUsage::from_io(100, 50);
    let u = abp_shim_openai::ir_usage_to_usage(&ir);
    assert_eq!(u.prompt_tokens, 100);
    assert_eq!(u.completion_tokens, 50);
    assert_eq!(u.total_tokens, 150);
}

// =========================================================================
// 7. OpenAI shim: tools_to_ir preserves definitions
// =========================================================================

#[test]
fn openai_tools_to_ir_preserves_definitions() {
    let tools = vec![abp_shim_openai::Tool::function(
        "search",
        "Search the web",
        json!({"type": "object", "properties": {"q": {"type": "string"}}}),
    )];
    let ir_tools = abp_shim_openai::tools_to_ir(&tools);
    assert_eq!(ir_tools.len(), 1);
    assert_eq!(ir_tools[0].name, "search");
    assert_eq!(ir_tools[0].description, "Search the web");
}

// =========================================================================
// 8. OpenAI shim: events_to_stream_events delta events
// =========================================================================

#[test]
fn openai_events_to_stream_events_produces_deltas() {
    let events = vec![
        agent_event_assistant_delta("Hel"),
        agent_event_assistant_delta("lo!"),
    ];
    let stream = abp_shim_openai::events_to_stream_events(&events, "gpt-4o");
    // role chunk + 2 deltas + stop chunk
    assert!(stream.len() >= 3);
    assert_eq!(
        stream.last().unwrap().choices[0].finish_reason.as_deref(),
        Some("stop")
    );
}

// =========================================================================
// 9. OpenAI shim: builder defaults model to gpt-4o
// =========================================================================

#[test]
fn openai_builder_defaults_model() {
    let req = abp_shim_openai::ChatCompletionRequest::builder()
        .messages(vec![abp_shim_openai::Message::user("hi")])
        .build();
    assert_eq!(req.model, "gpt-4o");
}

// =========================================================================
// 10. Claude shim: content_block roundtrip through IR
// =========================================================================

#[test]
fn claude_content_block_text_roundtrip() {
    let block = abp_shim_claude::ContentBlock::Text {
        text: "Hello".into(),
    };
    let ir = abp_shim_claude::content_block_to_ir(&block);
    let back = abp_shim_claude::content_block_from_ir(&ir);
    assert_eq!(block, back);
}

// =========================================================================
// 11. Claude shim: content_block tool_use roundtrip
// =========================================================================

#[test]
fn claude_content_block_tool_use_roundtrip() {
    let block = abp_shim_claude::ContentBlock::ToolUse {
        id: "tu_1".into(),
        name: "bash".into(),
        input: json!({"command": "ls"}),
    };
    let ir = abp_shim_claude::content_block_to_ir(&block);
    let back = abp_shim_claude::content_block_from_ir(&ir);
    assert_eq!(block, back);
}

// =========================================================================
// 12. Claude shim: content_block tool_result roundtrip
// =========================================================================

#[test]
fn claude_content_block_tool_result_roundtrip() {
    let block = abp_shim_claude::ContentBlock::ToolResult {
        tool_use_id: "tu_1".into(),
        content: Some("file.txt".into()),
        is_error: Some(false),
    };
    let ir = abp_shim_claude::content_block_to_ir(&block);
    let back = abp_shim_claude::content_block_from_ir(&ir);
    assert_eq!(block, back);
}

// =========================================================================
// 13. Claude shim: content_block thinking roundtrip
// =========================================================================

#[test]
fn claude_content_block_thinking_roundtrip() {
    let block = abp_shim_claude::ContentBlock::Thinking {
        thinking: "Let me think...".into(),
        signature: Some("sig_abc".into()),
    };
    let ir = abp_shim_claude::content_block_to_ir(&block);
    let back = abp_shim_claude::content_block_from_ir(&ir);
    assert_eq!(block, back);
}

// =========================================================================
// 14. Claude shim: content_block image base64 roundtrip
// =========================================================================

#[test]
fn claude_content_block_image_base64_roundtrip() {
    let block = abp_shim_claude::ContentBlock::Image {
        source: abp_shim_claude::ImageSource::Base64 {
            media_type: "image/png".into(),
            data: "iVBOR...".into(),
        },
    };
    let ir = abp_shim_claude::content_block_to_ir(&block);
    let back = abp_shim_claude::content_block_from_ir(&ir);
    assert_eq!(block, back);
}

// =========================================================================
// 15. Claude shim: content_block image URL roundtrip
// =========================================================================

#[test]
fn claude_content_block_image_url_roundtrip() {
    let block = abp_shim_claude::ContentBlock::Image {
        source: abp_shim_claude::ImageSource::Url {
            url: "https://example.com/img.png".into(),
        },
    };
    let ir = abp_shim_claude::content_block_to_ir(&block);
    let back = abp_shim_claude::content_block_from_ir(&ir);
    assert_eq!(block, back);
}

// =========================================================================
// 16. Claude shim: request_to_work_order preserves model
// =========================================================================

#[test]
fn claude_request_to_work_order_preserves_model() {
    let req = abp_shim_claude::MessageRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 1024,
        messages: vec![abp_shim_claude::Message {
            role: abp_shim_claude::Role::User,
            content: vec![abp_shim_claude::ContentBlock::Text {
                text: "Hello".into(),
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

// =========================================================================
// 17. Claude shim: request_to_claude preserves system prompt
// =========================================================================

#[test]
fn claude_request_to_claude_preserves_system() {
    let req = abp_shim_claude::MessageRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 4096,
        messages: vec![abp_shim_claude::Message {
            role: abp_shim_claude::Role::User,
            content: vec![abp_shim_claude::ContentBlock::Text { text: "hi".into() }],
        }],
        system: Some("You are a pirate.".into()),
        temperature: Some(0.3),
        stop_sequences: None,
        thinking: None,
        stream: None,
    };
    let claude_req = abp_shim_claude::request_to_claude(&req);
    assert_eq!(claude_req.system.as_deref(), Some("You are a pirate."));
    assert_eq!(claude_req.max_tokens, 4096);
}

// =========================================================================
// 18. Claude shim: response_from_events preserves assistant text
// =========================================================================

#[test]
fn claude_response_from_events_preserves_text() {
    let events = vec![agent_event_assistant_msg("Claude says hello!")];
    let resp = abp_shim_claude::response_from_events(&events, "claude-sonnet-4-20250514", None);

    assert_eq!(resp.model, "claude-sonnet-4-20250514");
    assert!(!resp.content.is_empty());
    match &resp.content[0] {
        abp_shim_claude::ContentBlock::Text { text } => {
            assert_eq!(text, "Claude says hello!");
        }
        _ => panic!("Expected text block"),
    }
}

// =========================================================================
// 19. Claude shim: response_from_events tool_call becomes tool_use
// =========================================================================

#[test]
fn claude_response_from_events_tool_call() {
    let events = vec![agent_event_tool_call(
        "bash",
        "tu_42",
        json!({"command": "echo hi"}),
    )];
    let resp = abp_shim_claude::response_from_events(&events, "claude-sonnet-4-20250514", None);

    assert!(resp.content.iter().any(|b| matches!(
        b,
        abp_shim_claude::ContentBlock::ToolUse { name, .. } if name == "bash"
    )));
    assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
}

// =========================================================================
// 20. Claude shim: response_from_claude roundtrip
// =========================================================================

#[test]
fn claude_response_from_claude_roundtrip() {
    use abp_claude_sdk::dialect::{ClaudeContentBlock, ClaudeResponse, ClaudeUsage};

    let resp = ClaudeResponse {
        id: "msg_test".into(),
        model: "claude-sonnet-4-20250514".into(),
        role: "assistant".into(),
        content: vec![ClaudeContentBlock::Text {
            text: "Hello!".into(),
        }],
        stop_reason: Some("end_turn".into()),
        usage: Some(ClaudeUsage {
            input_tokens: 10,
            output_tokens: 5,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        }),
    };

    let shim_resp = abp_shim_claude::response_from_claude(&resp);
    assert_eq!(shim_resp.id, "msg_test");
    assert_eq!(shim_resp.model, "claude-sonnet-4-20250514");
    assert_eq!(shim_resp.usage.input_tokens, 10);
    assert_eq!(shim_resp.usage.output_tokens, 5);
    assert_eq!(shim_resp.content.len(), 1);
    match &shim_resp.content[0] {
        abp_shim_claude::ContentBlock::Text { text } => assert_eq!(text, "Hello!"),
        _ => panic!("Expected text block"),
    }
}

// =========================================================================
// 21. Claude shim: serde roundtrip of ContentBlock
// =========================================================================

#[test]
fn claude_content_block_serde_roundtrip() {
    let block = abp_shim_claude::ContentBlock::ToolUse {
        id: "tu_1".into(),
        name: "bash".into(),
        input: json!({"command": "ls -la"}),
    };
    let json_str = serde_json::to_string(&block).unwrap();
    let back: abp_shim_claude::ContentBlock = serde_json::from_str(&json_str).unwrap();
    assert_eq!(block, back);
}

// =========================================================================
// 22. Gemini shim: Part serde roundtrip (text)
// =========================================================================

#[test]
fn gemini_part_text_serde_roundtrip() {
    let part = abp_shim_gemini::Part::text("Hello Gemini!");
    let json_str = serde_json::to_string(&part).unwrap();
    let back: abp_shim_gemini::Part = serde_json::from_str(&json_str).unwrap();
    assert_eq!(part, back);
}

// =========================================================================
// 23. Gemini shim: Part serde roundtrip (function_call)
// =========================================================================

#[test]
fn gemini_part_function_call_serde_roundtrip() {
    let part = abp_shim_gemini::Part::function_call("search", json!({"q": "rust"}));
    let json_str = serde_json::to_string(&part).unwrap();
    let back: abp_shim_gemini::Part = serde_json::from_str(&json_str).unwrap();
    assert_eq!(part, back);
}

// =========================================================================
// 24. Gemini shim: Part serde roundtrip (inline_data)
// =========================================================================

#[test]
fn gemini_part_inline_data_serde_roundtrip() {
    let part = abp_shim_gemini::Part::inline_data("image/png", "base64data");
    let json_str = serde_json::to_string(&part).unwrap();
    let back: abp_shim_gemini::Part = serde_json::from_str(&json_str).unwrap();
    assert_eq!(part, back);
}

// =========================================================================
// 25. Gemini shim: Part serde roundtrip (function_response)
// =========================================================================

#[test]
fn gemini_part_function_response_serde_roundtrip() {
    let part = abp_shim_gemini::Part::function_response("search", json!({"results": []}));
    let json_str = serde_json::to_string(&part).unwrap();
    let back: abp_shim_gemini::Part = serde_json::from_str(&json_str).unwrap();
    assert_eq!(part, back);
}

// =========================================================================
// 26. Gemini shim: usage_to_ir and usage_from_ir roundtrip
// =========================================================================

#[test]
fn gemini_usage_ir_roundtrip() {
    let usage = abp_shim_gemini::UsageMetadata {
        prompt_token_count: 100,
        candidates_token_count: 50,
        total_token_count: 150,
    };
    let ir = abp_shim_gemini::usage_to_ir(&usage);
    assert_eq!(ir.input_tokens, 100);
    assert_eq!(ir.output_tokens, 50);
    assert_eq!(ir.total_tokens, 150);

    let back = abp_shim_gemini::usage_from_ir(&ir);
    assert_eq!(back, usage);
}

// =========================================================================
// 27. Gemini shim: to_dialect_request preserves model and contents
// =========================================================================

#[test]
fn gemini_to_dialect_request_preserves_model() {
    let req = abp_shim_gemini::GenerateContentRequest::new("gemini-2.5-flash").add_content(
        abp_shim_gemini::Content::user(vec![abp_shim_gemini::Part::text("Hello")]),
    );

    let dialect_req = abp_shim_gemini::to_dialect_request(&req);
    assert_eq!(dialect_req.model, "gemini-2.5-flash");
    assert_eq!(dialect_req.contents.len(), 1);
}

// =========================================================================
// 28. Gemini shim: from_dialect_response preserves candidates
// =========================================================================

#[test]
fn gemini_from_dialect_response_preserves_candidates() {
    use abp_gemini_sdk::dialect::{
        GeminiCandidate, GeminiContent, GeminiPart, GeminiResponse, GeminiUsageMetadata,
    };

    let resp = GeminiResponse {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("Hi there!".into())],
            },
            finish_reason: Some("STOP".into()),
            safety_ratings: None,
            citation_metadata: None,
        }],
        usage_metadata: Some(GeminiUsageMetadata {
            prompt_token_count: 5,
            candidates_token_count: 3,
            total_token_count: 8,
        }),
    };

    let shim_resp = abp_shim_gemini::from_dialect_response(&resp);
    assert_eq!(shim_resp.candidates.len(), 1);
    assert_eq!(shim_resp.text(), Some("Hi there!"));
    let usage = shim_resp.usage_metadata.unwrap();
    assert_eq!(usage.prompt_token_count, 5);
    assert_eq!(usage.candidates_token_count, 3);
    assert_eq!(usage.total_token_count, 8);
}

// =========================================================================
// 29. Gemini shim: from_dialect_stream_chunk preserves text
// =========================================================================

#[test]
fn gemini_from_dialect_stream_chunk_preserves_text() {
    use abp_gemini_sdk::dialect::{GeminiCandidate, GeminiContent, GeminiPart, GeminiStreamChunk};

    let chunk = GeminiStreamChunk {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("Hello".into())],
            },
            finish_reason: None,
            safety_ratings: None,
            citation_metadata: None,
        }],
        usage_metadata: None,
    };

    let event = abp_shim_gemini::from_dialect_stream_chunk(&chunk);
    assert_eq!(event.text(), Some("Hello"));
}

// =========================================================================
// 30. Codex shim: request_to_ir preserves user message
// =========================================================================

#[test]
fn codex_request_to_ir_preserves_user_message() {
    let req = abp_shim_codex::CodexRequestBuilder::new()
        .model("codex-mini-latest")
        .input(vec![abp_shim_codex::codex_message("user", "Fix my code")])
        .build();

    let conv = abp_shim_codex::request_to_ir(&req);
    assert!(!conv.is_empty());
    let user_msgs = conv.messages_by_role(IrRole::User);
    assert!(!user_msgs.is_empty());
    assert_eq!(user_msgs[0].text_content(), "Fix my code");
}

// =========================================================================
// 31. Codex shim: request_to_work_order preserves model
// =========================================================================

#[test]
fn codex_request_to_work_order_preserves_model() {
    let req = abp_shim_codex::CodexRequestBuilder::new()
        .model("codex-mini-latest")
        .input(vec![abp_shim_codex::codex_message("user", "test")])
        .build();

    let wo = abp_shim_codex::request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("codex-mini-latest"));
}

// =========================================================================
// 32. Codex shim: receipt_to_response assistant message
// =========================================================================

#[test]
fn codex_receipt_to_response_preserves_text() {
    let events = vec![agent_event_assistant_msg("Codex response!")];
    let receipt = abp_shim_codex::mock_receipt(events);
    let resp = abp_shim_codex::receipt_to_response(&receipt, "codex-mini-latest");

    assert_eq!(resp.model, "codex-mini-latest");
    assert!(!resp.output.is_empty());
}

// =========================================================================
// 33. Codex shim: receipt_to_response tool call
// =========================================================================

#[test]
fn codex_receipt_to_response_preserves_tool_call() {
    let events = vec![agent_event_tool_call(
        "exec",
        "fc_1",
        json!({"code": "print('hi')"}),
    )];
    let receipt = abp_shim_codex::mock_receipt(events);
    let resp = abp_shim_codex::receipt_to_response(&receipt, "codex-mini-latest");

    // Find the function call item
    let has_fc = resp.output.iter().any(|item| {
        matches!(item, abp_codex_sdk::dialect::CodexResponseItem::FunctionCall { name, .. } if name == "exec")
    });
    assert!(has_fc, "Expected function call in output");
}

// =========================================================================
// 34. Codex shim: ir_usage_to_usage
// =========================================================================

#[test]
fn codex_ir_usage_to_usage_maps_fields() {
    let ir = IrUsage::from_io(200, 100);
    let u = abp_shim_codex::ir_usage_to_usage(&ir);
    assert_eq!(u.input_tokens, 200);
    assert_eq!(u.output_tokens, 100);
    assert_eq!(u.total_tokens, 300);
}

// =========================================================================
// 35. Codex shim: events_to_stream_events structure
// =========================================================================

#[test]
fn codex_events_to_stream_events_bookends() {
    let events = vec![agent_event_assistant_msg("hi")];
    let stream = abp_shim_codex::events_to_stream_events(&events, "codex-mini-latest");

    // Should start with ResponseCreated and end with ResponseCompleted
    assert!(matches!(
        &stream[0],
        abp_codex_sdk::dialect::CodexStreamEvent::ResponseCreated { .. }
    ));
    assert!(matches!(
        stream.last().unwrap(),
        abp_codex_sdk::dialect::CodexStreamEvent::ResponseCompleted { .. }
    ));
}

// =========================================================================
// 36. Codex shim: builder defaults model
// =========================================================================

#[test]
fn codex_builder_defaults_model() {
    let req = abp_shim_codex::CodexRequestBuilder::new()
        .input(vec![abp_shim_codex::codex_message("user", "test")])
        .build();
    assert_eq!(req.model, "codex-mini-latest");
}

// =========================================================================
// 37. Kimi shim: request_to_ir preserves roles
// =========================================================================

#[test]
fn kimi_request_to_ir_preserves_roles() {
    let req = abp_shim_kimi::KimiRequestBuilder::new()
        .model("moonshot-v1-8k")
        .messages(vec![
            abp_shim_kimi::Message::system("Be concise."),
            abp_shim_kimi::Message::user("Hello"),
        ])
        .build();

    let conv = abp_shim_kimi::request_to_ir(&req);
    assert_eq!(conv.len(), 2);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[1].role, IrRole::User);
}

// =========================================================================
// 38. Kimi shim: messages_to_ir roundtrip
// =========================================================================

#[test]
fn kimi_messages_to_ir_and_back() {
    let msgs = vec![
        abp_shim_kimi::Message::system("System"),
        abp_shim_kimi::Message::user("User msg"),
        abp_shim_kimi::Message::assistant("Reply"),
    ];
    let conv = abp_shim_kimi::messages_to_ir(&msgs);
    let back = abp_shim_kimi::ir_to_messages(&conv);

    assert_eq!(back.len(), 3);
    assert_eq!(back[0].role, "system");
    assert_eq!(back[0].content.as_deref(), Some("System"));
    assert_eq!(back[1].role, "user");
    assert_eq!(back[2].role, "assistant");
}

// =========================================================================
// 39. Kimi shim: request_to_work_order preserves model
// =========================================================================

#[test]
fn kimi_request_to_work_order_preserves_model() {
    let req = abp_shim_kimi::KimiRequestBuilder::new()
        .model("moonshot-v1-128k")
        .messages(vec![abp_shim_kimi::Message::user("test")])
        .build();

    let wo = abp_shim_kimi::request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-128k"));
}

// =========================================================================
// 40. Kimi shim: receipt_to_response assistant message
// =========================================================================

#[test]
fn kimi_receipt_to_response_preserves_text() {
    let events = vec![agent_event_assistant_msg("Kimi reply!")];
    let receipt = abp_shim_kimi::mock_receipt(events);
    let resp = abp_shim_kimi::receipt_to_response(&receipt, "moonshot-v1-8k");

    assert_eq!(resp.model, "moonshot-v1-8k");
    assert!(!resp.choices.is_empty());
    assert_eq!(
        resp.choices[0].message.content.as_deref(),
        Some("Kimi reply!")
    );
}

// =========================================================================
// 41. Kimi shim: receipt_to_response tool call
// =========================================================================

#[test]
fn kimi_receipt_to_response_preserves_tool_call() {
    let events = vec![agent_event_tool_call(
        "search",
        "call_k1",
        json!({"query": "Rust async"}),
    )];
    let receipt = abp_shim_kimi::mock_receipt(events);
    let resp = abp_shim_kimi::receipt_to_response(&receipt, "moonshot-v1-8k");

    let tc = resp.choices[0].message.tool_calls.as_ref().unwrap();
    assert_eq!(tc.len(), 1);
    assert_eq!(tc[0].function.name, "search");
    assert!(tc[0].function.arguments.contains("Rust async"));
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
}

// =========================================================================
// 42. Kimi shim: ir_usage_to_usage
// =========================================================================

#[test]
fn kimi_ir_usage_to_usage_maps_fields() {
    let ir = IrUsage::from_io(300, 150);
    let u = abp_shim_kimi::ir_usage_to_usage(&ir);
    assert_eq!(u.prompt_tokens, 300);
    assert_eq!(u.completion_tokens, 150);
    assert_eq!(u.total_tokens, 450);
}

// =========================================================================
// 43. Kimi shim: events_to_stream_chunks produces chunks with stop
// =========================================================================

#[test]
fn kimi_events_to_stream_chunks_ends_with_stop() {
    let events = vec![agent_event_assistant_delta("Hi")];
    let chunks = abp_shim_kimi::events_to_stream_chunks(&events, "moonshot-v1-8k");

    // At least one delta + one stop chunk
    assert!(chunks.len() >= 2);
    let last = chunks.last().unwrap();
    assert_eq!(last.choices[0].finish_reason.as_deref(), Some("stop"));
}

// =========================================================================
// 44. Kimi shim: builder defaults model
// =========================================================================

#[test]
fn kimi_builder_defaults_model() {
    let req = abp_shim_kimi::KimiRequestBuilder::new()
        .messages(vec![abp_shim_kimi::Message::user("test")])
        .build();
    assert_eq!(req.model, "moonshot-v1-8k");
}

// =========================================================================
// 45. Copilot shim: request_to_ir preserves roles
// =========================================================================

#[test]
fn copilot_request_to_ir_preserves_roles() {
    let req = abp_shim_copilot::CopilotRequestBuilder::new()
        .messages(vec![
            abp_shim_copilot::Message::system("Be brief."),
            abp_shim_copilot::Message::user("Hello"),
        ])
        .build();

    let conv = abp_shim_copilot::request_to_ir(&req);
    assert_eq!(conv.len(), 2);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[1].role, IrRole::User);
}

// =========================================================================
// 46. Copilot shim: messages_to_ir roundtrip
// =========================================================================

#[test]
fn copilot_messages_to_ir_and_back() {
    let msgs = vec![
        abp_shim_copilot::Message::system("System prompt"),
        abp_shim_copilot::Message::user("User message"),
        abp_shim_copilot::Message::assistant("Reply"),
    ];
    let conv = abp_shim_copilot::messages_to_ir(&msgs);
    let back = abp_shim_copilot::ir_to_messages(&conv);

    assert_eq!(back.len(), 3);
    assert_eq!(back[0].role, "system");
    assert_eq!(back[0].content, "System prompt");
    assert_eq!(back[1].role, "user");
    assert_eq!(back[2].role, "assistant");
}

// =========================================================================
// 47. Copilot shim: request_to_work_order preserves model
// =========================================================================

#[test]
fn copilot_request_to_work_order_preserves_model() {
    let req = abp_shim_copilot::CopilotRequestBuilder::new()
        .model("gpt-4-turbo")
        .messages(vec![abp_shim_copilot::Message::user("test")])
        .build();

    let wo = abp_shim_copilot::request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4-turbo"));
}

// =========================================================================
// 48. Copilot shim: receipt_to_response preserves text
// =========================================================================

#[test]
fn copilot_receipt_to_response_preserves_text() {
    let events = vec![agent_event_assistant_msg("Copilot reply!")];
    let receipt = abp_shim_copilot::mock_receipt(events);
    let resp = abp_shim_copilot::receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.message, "Copilot reply!");
    assert!(resp.copilot_errors.is_empty());
}

// =========================================================================
// 49. Copilot shim: receipt_to_response with tool call
// =========================================================================

#[test]
fn copilot_receipt_to_response_tool_call() {
    let events = vec![agent_event_tool_call(
        "read_file",
        "call_abc",
        json!({"path": "src/main.rs"}),
    )];
    let receipt = abp_shim_copilot::mock_receipt(events);
    let resp = abp_shim_copilot::receipt_to_response(&receipt, "gpt-4o");

    let fc = resp.function_call.unwrap();
    assert_eq!(fc.name, "read_file");
    assert!(fc.arguments.contains("main.rs"));
}

// =========================================================================
// 50. Copilot shim: events_to_stream_events structure
// =========================================================================

#[test]
fn copilot_events_to_stream_events_structure() {
    use abp_copilot_sdk::dialect::CopilotStreamEvent;

    let events = vec![
        agent_event_assistant_delta("Hi"),
        agent_event_assistant_delta("!"),
    ];
    let stream = abp_shim_copilot::events_to_stream_events(&events, "gpt-4o");

    // references + 2 deltas + done = 4
    assert_eq!(stream.len(), 4);
    assert!(matches!(
        &stream[0],
        CopilotStreamEvent::CopilotReferences { .. }
    ));
    assert!(matches!(&stream[1], CopilotStreamEvent::TextDelta { .. }));
    assert!(matches!(&stream[2], CopilotStreamEvent::TextDelta { .. }));
    assert!(matches!(&stream[3], CopilotStreamEvent::Done {}));
}

// =========================================================================
// 51. Copilot shim: ir_usage_to_tuple
// =========================================================================

#[test]
fn copilot_ir_usage_to_tuple_maps_fields() {
    let ir = IrUsage::from_io(50, 25);
    let (input, output, total) = abp_shim_copilot::ir_usage_to_tuple(&ir);
    assert_eq!(input, 50);
    assert_eq!(output, 25);
    assert_eq!(total, 75);
}

// =========================================================================
// 52. Copilot shim: response_to_ir roundtrip
// =========================================================================

#[test]
fn copilot_response_to_ir_preserves_assistant() {
    use abp_copilot_sdk::dialect::CopilotResponse;

    let resp = CopilotResponse {
        message: "Done!".into(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: None,
        function_call: None,
    };
    let conv = abp_shim_copilot::response_to_ir(&resp);
    assert_eq!(conv.len(), 1);
    assert_eq!(conv.messages[0].role, IrRole::Assistant);
    assert_eq!(conv.messages[0].text_content(), "Done!");
}

// =========================================================================
// 53. Copilot shim: empty response_to_ir returns empty
// =========================================================================

#[test]
fn copilot_empty_response_to_ir_returns_empty() {
    use abp_copilot_sdk::dialect::CopilotResponse;

    let resp = CopilotResponse {
        message: String::new(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: None,
        function_call: None,
    };
    let conv = abp_shim_copilot::response_to_ir(&resp);
    assert!(conv.is_empty());
}

// =========================================================================
// 54. Cross-shim: OpenAI and Kimi share compatible IR for user messages
// =========================================================================

#[test]
fn openai_kimi_ir_user_message_compat() {
    let openai_msgs = vec![abp_shim_openai::Message::user("Hello world")];
    let kimi_msgs = vec![abp_shim_kimi::Message::user("Hello world")];

    let openai_ir = abp_shim_openai::messages_to_ir(&openai_msgs);
    let kimi_ir = abp_shim_kimi::messages_to_ir(&kimi_msgs);

    assert_eq!(openai_ir.len(), kimi_ir.len());
    assert_eq!(
        openai_ir.messages[0].text_content(),
        kimi_ir.messages[0].text_content()
    );
    assert_eq!(openai_ir.messages[0].role, kimi_ir.messages[0].role);
}

// =========================================================================
// 55. Cross-shim: OpenAI and Copilot share compatible IR
// =========================================================================

#[test]
fn openai_copilot_ir_user_message_compat() {
    let openai_msgs = vec![abp_shim_openai::Message::user("Test message")];
    let copilot_msgs = vec![abp_shim_copilot::Message::user("Test message")];

    let openai_ir = abp_shim_openai::messages_to_ir(&openai_msgs);
    let copilot_ir = abp_shim_copilot::messages_to_ir(&copilot_msgs);

    assert_eq!(openai_ir.len(), copilot_ir.len());
    assert_eq!(
        openai_ir.messages[0].text_content(),
        copilot_ir.messages[0].text_content()
    );
}

// =========================================================================
// 56. Cross-shim: error events surface in all shims
// =========================================================================

#[test]
fn error_events_surface_in_openai_and_kimi() {
    let events = vec![agent_event_error("rate limit exceeded")];

    // OpenAI: error should appear in response (we just check it doesn't panic)
    let receipt_oai = abp_shim_openai::mock_receipt(events.clone());
    let resp_oai = abp_shim_openai::receipt_to_response(&receipt_oai, "gpt-4o");
    // OpenAI puts errors as text in the message
    assert!(
        resp_oai.choices[0]
            .message
            .content
            .as_deref()
            .unwrap_or("")
            .contains("rate limit")
    );

    // Kimi: error should appear in response content
    let receipt_kimi = abp_shim_kimi::mock_receipt(events);
    let resp_kimi = abp_shim_kimi::receipt_to_response(&receipt_kimi, "moonshot-v1-8k");
    assert!(
        resp_kimi.choices[0]
            .message
            .content
            .as_deref()
            .unwrap_or("")
            .contains("rate limit")
    );
}

// =========================================================================
// 57. Cross-shim: error events surface in Copilot
// =========================================================================

#[test]
fn error_events_surface_in_copilot() {
    let events = vec![agent_event_error("timeout")];
    let receipt = abp_shim_copilot::mock_receipt(events);
    let resp = abp_shim_copilot::receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.copilot_errors.len(), 1);
    assert!(resp.copilot_errors[0].message.contains("timeout"));
}

// =========================================================================
// 58. OpenAI shim: Message serde roundtrip
// =========================================================================

#[test]
fn openai_message_serde_roundtrip() {
    let msg = abp_shim_openai::Message::user("Hello!");
    let json_str = serde_json::to_string(&msg).unwrap();
    let back: abp_shim_openai::Message = serde_json::from_str(&json_str).unwrap();
    assert_eq!(back.role, abp_shim_openai::Role::User);
    assert_eq!(back.content.as_deref(), Some("Hello!"));
}

// =========================================================================
// 59. Kimi shim: Message serde roundtrip
// =========================================================================

#[test]
fn kimi_message_serde_roundtrip() {
    let msg = abp_shim_kimi::Message::user("Test Kimi");
    let json_str = serde_json::to_string(&msg).unwrap();
    let back: abp_shim_kimi::Message = serde_json::from_str(&json_str).unwrap();
    assert_eq!(back.role, "user");
    assert_eq!(back.content.as_deref(), Some("Test Kimi"));
}

// =========================================================================
// 60. Copilot shim: Message serde roundtrip
// =========================================================================

#[test]
fn copilot_message_serde_roundtrip() {
    let msg = abp_shim_copilot::Message::system("System prompt");
    let json_str = serde_json::to_string(&msg).unwrap();
    let back: abp_shim_copilot::Message = serde_json::from_str(&json_str).unwrap();
    assert_eq!(back.role, "system");
    assert_eq!(back.content, "System prompt");
}

// =========================================================================
// 61. OpenAI shim: receipt with deltas concatenates text
// =========================================================================

#[test]
fn openai_receipt_concatenates_deltas() {
    let events = vec![
        agent_event_assistant_delta("Hel"),
        agent_event_assistant_delta("lo "),
        agent_event_assistant_delta("world!"),
    ];
    let receipt = abp_shim_openai::mock_receipt(events);
    let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(
        resp.choices[0].message.content.as_deref(),
        Some("Hello world!")
    );
}

// =========================================================================
// 62. Kimi shim: receipt with deltas concatenates text
// =========================================================================

#[test]
fn kimi_receipt_concatenates_deltas() {
    let events = vec![
        agent_event_assistant_delta("Hel"),
        agent_event_assistant_delta("lo!"),
    ];
    let receipt = abp_shim_kimi::mock_receipt(events);
    let resp = abp_shim_kimi::receipt_to_response(&receipt, "moonshot-v1-8k");
    assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hello!"));
}

// =========================================================================
// 63. Gemini shim: GenerateContentResponse text extraction
// =========================================================================

#[test]
fn gemini_response_text_extraction() {
    let resp = abp_shim_gemini::GenerateContentResponse {
        candidates: vec![abp_shim_gemini::Candidate {
            content: abp_shim_gemini::Content::model(vec![abp_shim_gemini::Part::text(
                "Hello from Gemini!",
            )]),
            finish_reason: Some("STOP".into()),
        }],
        usage_metadata: None,
    };
    assert_eq!(resp.text(), Some("Hello from Gemini!"));
}

// =========================================================================
// 64. Gemini shim: function_calls extraction
// =========================================================================

#[test]
fn gemini_response_function_calls_extraction() {
    let resp = abp_shim_gemini::GenerateContentResponse {
        candidates: vec![abp_shim_gemini::Candidate {
            content: abp_shim_gemini::Content::model(vec![abp_shim_gemini::Part::function_call(
                "search",
                json!({"q": "rust"}),
            )]),
            finish_reason: None,
        }],
        usage_metadata: None,
    };
    let fcs = resp.function_calls();
    assert_eq!(fcs.len(), 1);
    assert_eq!(fcs[0].0, "search");
    assert_eq!(fcs[0].1, &json!({"q": "rust"}));
}

// =========================================================================
// 65. Codex shim: response_to_ir roundtrip for assistant message
// =========================================================================

#[test]
fn codex_response_to_ir_preserves_assistant_text() {
    let events = vec![agent_event_assistant_msg("Hello from Codex!")];
    let receipt = abp_shim_codex::mock_receipt(events);
    let resp = abp_shim_codex::receipt_to_response(&receipt, "codex-mini-latest");

    let conv = abp_shim_codex::response_to_ir(&resp);
    assert!(!conv.is_empty());
    let assistant = conv.last_assistant().expect("should have assistant msg");
    assert!(assistant.text_content().contains("Hello from Codex!"));
}
