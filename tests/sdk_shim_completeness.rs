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
//! SDK shim surface completeness tests.
//!
//! Verifies that each shim crate exposes the expected API surface, accepts
//! the right input types, and produces the right output types.

use abp_core::ir::{IrConversation, IrMessage, IrRole, IrUsage};
use abp_core::{AgentEvent, AgentEventKind};
use abp_dialect::Dialect;
use abp_mapper::{default_ir_mapper, supported_ir_pairs};
use chrono::Utc;
use serde_json::json;

// ── Helpers ─────────────────────────────────────────────────────────────

fn ts() -> chrono::DateTime<Utc> {
    Utc::now()
}

fn assistant_message_event(text: &str) -> AgentEvent {
    AgentEvent {
        ts: ts(),
        kind: AgentEventKind::AssistantMessage {
            text: text.to_string(),
        },
        ext: None,
    }
}

fn tool_call_event(name: &str, id: &str) -> AgentEvent {
    AgentEvent {
        ts: ts(),
        kind: AgentEventKind::ToolCall {
            tool_name: name.to_string(),
            tool_use_id: Some(id.to_string()),
            parent_tool_use_id: None,
            input: json!({"arg": "value"}),
        },
        ext: None,
    }
}

fn run_started_event() -> AgentEvent {
    AgentEvent {
        ts: ts(),
        kind: AgentEventKind::RunStarted {
            message: "started".into(),
        },
        ext: None,
    }
}

fn run_completed_event() -> AgentEvent {
    AgentEvent {
        ts: ts(),
        kind: AgentEventKind::RunCompleted {
            message: "done".into(),
        },
        ext: None,
    }
}

fn sample_ir_usage() -> IrUsage {
    IrUsage {
        input_tokens: 100,
        output_tokens: 50,
        total_tokens: 150,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  1. OpenAI shim surface (12 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_chat_completion_request_builder() {
    use abp_shim_openai::{ChatCompletionRequest, Message};

    let req = ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![Message::user("Hello")])
        .build();

    assert_eq!(req.model, "gpt-4o");
    assert_eq!(req.messages.len(), 1);
}

#[test]
fn openai_function_calling_tools() {
    use abp_shim_openai::{Tool, tools_to_ir};

    let tools = vec![Tool::function(
        "get_weather",
        "Get the weather",
        json!({"type": "object", "properties": {"city": {"type": "string"}}}),
    )];

    let ir_tools = tools_to_ir(&tools);
    assert_eq!(ir_tools.len(), 1);
    assert_eq!(ir_tools[0].name, "get_weather");
    assert_eq!(ir_tools[0].description, "Get the weather");
}

#[test]
fn openai_request_to_ir_conversion() {
    use abp_shim_openai::{ChatCompletionRequest, Message, request_to_ir};

    let req = ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![
            Message::system("You are a helper"),
            Message::user("What is Rust?"),
        ])
        .build();

    let conv: IrConversation = request_to_ir(&req);
    assert!(!conv.is_empty());
}

#[test]
fn openai_request_to_work_order() {
    use abp_shim_openai::{ChatCompletionRequest, Message, request_to_work_order};

    let req = ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![Message::user("Explain closures")])
        .temperature(0.7)
        .max_tokens(512)
        .build();

    let wo = request_to_work_order(&req);
    assert!(!wo.task.is_empty());
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
}

#[test]
fn openai_receipt_to_response() {
    use abp_shim_openai::{mock_receipt, receipt_to_response};

    let receipt = mock_receipt(vec![
        run_started_event(),
        assistant_message_event("Hello!"),
        run_completed_event(),
    ]);

    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.object, "chat.completion");
    assert_eq!(resp.model, "gpt-4o");
    assert_eq!(resp.choices.len(), 1);
    assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hello!"));
}

#[test]
fn openai_events_to_stream_events() {
    use abp_shim_openai::events_to_stream_events;

    let events = vec![assistant_message_event("Hi")];
    let stream = events_to_stream_events(&events, "gpt-4o");
    assert!(!stream.is_empty());
}

#[test]
fn openai_messages_roundtrip_ir() {
    use abp_shim_openai::{Message, ir_to_messages, messages_to_ir};

    let messages = vec![Message::user("Hello"), Message::assistant("World")];

    let conv = messages_to_ir(&messages);
    assert!(!conv.is_empty());

    let back = ir_to_messages(&conv);
    assert!(!back.is_empty());
}

#[test]
fn openai_ir_usage_conversion() {
    use abp_shim_openai::ir_usage_to_usage;

    let usage = ir_usage_to_usage(&sample_ir_usage());
    assert_eq!(usage.prompt_tokens, 100);
    assert_eq!(usage.completion_tokens, 50);
    assert_eq!(usage.total_tokens, 150);
}

#[test]
fn openai_system_message_handling() {
    use abp_shim_openai::{ChatCompletionRequest, Message, request_to_ir};

    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::system("Be concise"), Message::user("Hi")])
        .build();

    let conv = request_to_ir(&req);
    let system_msgs = conv.messages_by_role(IrRole::System);
    assert!(!system_msgs.is_empty());
}

#[test]
fn openai_streaming_flag_in_builder() {
    use abp_shim_openai::{ChatCompletionRequest, Message};

    let req = ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![Message::user("test")])
        .stream(true)
        .build();

    assert_eq!(req.stream, Some(true));
}

#[test]
fn openai_tool_calls_in_response() {
    use abp_shim_openai::{mock_receipt, receipt_to_response};

    let receipt = mock_receipt(vec![
        run_started_event(),
        tool_call_event("get_weather", "call_1"),
        run_completed_event(),
    ]);

    let resp = receipt_to_response(&receipt, "gpt-4o");
    let msg = &resp.choices[0].message;
    assert!(msg.tool_calls.is_some());
    let tc = msg.tool_calls.as_ref().unwrap();
    assert_eq!(tc[0].function.name, "get_weather");
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
}

#[test]
fn openai_json_mode_response_format() {
    use abp_shim_openai::{ChatCompletionRequest, Message, ResponseFormat};

    let req = ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![Message::user("give json")])
        .response_format(ResponseFormat::JsonObject)
        .build();

    assert!(req.response_format.is_some());
}

// ═══════════════════════════════════════════════════════════════════════
//  2. Claude shim surface (12 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn claude_message_request_construction() {
    use abp_shim_claude::{ContentBlock, Message, MessageRequest, Role};

    let req = MessageRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 1024,
        messages: vec![Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "Hello".into(),
            }],
        }],
        system: Some("Be helpful".into()),
        temperature: Some(0.5),
        stop_sequences: None,
        thinking: None,
        stream: None,
    };

    assert_eq!(req.model, "claude-sonnet-4-20250514");
    assert_eq!(req.max_tokens, 1024);
    assert_eq!(req.system.as_deref(), Some("Be helpful"));
}

#[test]
fn claude_tool_use_content_block() {
    use abp_shim_claude::ContentBlock;

    let block = ContentBlock::ToolUse {
        id: "tu_1".into(),
        name: "search".into(),
        input: json!({"query": "rust lang"}),
    };

    let json = serde_json::to_value(&block).unwrap();
    assert_eq!(json["type"], "tool_use");
    assert_eq!(json["name"], "search");
}

#[test]
fn claude_thinking_content_block() {
    use abp_shim_claude::ContentBlock;

    let block = ContentBlock::Thinking {
        thinking: "Let me reason...".into(),
        signature: Some("sig123".into()),
    };

    let json = serde_json::to_value(&block).unwrap();
    assert_eq!(json["type"], "thinking");
    assert_eq!(json["thinking"], "Let me reason...");
}

#[test]
fn claude_system_prompt_in_request() {
    use abp_shim_claude::{ContentBlock, Message, MessageRequest, Role, request_to_work_order};

    let req = MessageRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 1024,
        messages: vec![Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "Hello".into(),
            }],
        }],
        system: Some("You are a coding expert".into()),
        temperature: None,
        stop_sequences: None,
        thinking: None,
        stream: None,
    };

    let wo = request_to_work_order(&req);
    assert!(!wo.task.is_empty());
}

#[test]
fn claude_content_block_ir_roundtrip() {
    use abp_shim_claude::{ContentBlock, content_block_from_ir, content_block_to_ir};

    let block = ContentBlock::Text {
        text: "test".into(),
    };
    let ir = content_block_to_ir(&block);
    let back = content_block_from_ir(&ir);
    assert_eq!(block, back);
}

#[test]
fn claude_message_to_ir() {
    use abp_shim_claude::{ContentBlock, Message, Role, message_to_ir};

    let msg = Message {
        role: Role::User,
        content: vec![ContentBlock::Text {
            text: "Hello".into(),
        }],
    };

    let ir = message_to_ir(&msg);
    assert_eq!(ir.role, "user");
}

#[test]
fn claude_request_to_claude_sdk() {
    use abp_shim_claude::{ContentBlock, Message, MessageRequest, Role, request_to_claude};

    let req = MessageRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 512,
        messages: vec![Message {
            role: Role::User,
            content: vec![ContentBlock::Text { text: "Hi".into() }],
        }],
        system: None,
        temperature: None,
        stop_sequences: None,
        thinking: None,
        stream: None,
    };

    let claude_req = request_to_claude(&req);
    assert_eq!(claude_req.model, "claude-sonnet-4-20250514");
    assert_eq!(claude_req.max_tokens, 512);
}

#[test]
fn claude_request_to_work_order() {
    use abp_shim_claude::{ContentBlock, Message, MessageRequest, Role, request_to_work_order};

    let req = MessageRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 1024,
        messages: vec![Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "Explain generics".into(),
            }],
        }],
        system: None,
        temperature: Some(0.7),
        stop_sequences: None,
        thinking: None,
        stream: None,
    };

    let wo = request_to_work_order(&req);
    assert!(!wo.task.is_empty());
}

#[test]
fn claude_response_from_events() {
    use abp_shim_claude::response_from_events;

    let events = vec![assistant_message_event("Generics allow...")];

    let resp = response_from_events(&events, "claude-sonnet-4-20250514", None);
    assert_eq!(resp.role, "assistant");
    assert!(!resp.content.is_empty());
}

#[test]
fn claude_stream_event_variants() {
    use abp_shim_claude::StreamEvent;

    // Verify the enum variants exist and are constructable
    let _start = StreamEvent::MessageStart {
        message: abp_shim_claude::MessageResponse {
            id: "msg_1".into(),
            response_type: "message".into(),
            role: "assistant".into(),
            content: vec![],
            model: "claude-sonnet-4-20250514".into(),
            stop_reason: None,
            stop_sequence: None,
            usage: abp_shim_claude::Usage {
                input_tokens: 0,
                output_tokens: 0,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
        },
    };

    let _stop = StreamEvent::MessageStop {};
    let _ping = StreamEvent::Ping {};
}

#[test]
fn claude_image_source_handling() {
    use abp_shim_claude::{ContentBlock, ImageSource, content_block_from_ir, content_block_to_ir};

    let block = ContentBlock::Image {
        source: ImageSource::Base64 {
            media_type: "image/png".into(),
            data: "iVBORw0KGgo=".into(),
        },
    };

    let ir = content_block_to_ir(&block);
    let back = content_block_from_ir(&ir);
    assert_eq!(block, back);
}

#[test]
fn claude_tool_result_content_block() {
    use abp_shim_claude::ContentBlock;

    let block = ContentBlock::ToolResult {
        tool_use_id: "tu_1".into(),
        content: Some("42 degrees".into()),
        is_error: Some(false),
    };

    let json = serde_json::to_value(&block).unwrap();
    assert_eq!(json["type"], "tool_result");
    assert_eq!(json["tool_use_id"], "tu_1");
}

// ═══════════════════════════════════════════════════════════════════════
//  3. Gemini shim surface (12 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn gemini_generate_content_request() {
    use abp_shim_gemini::{Content, GenerateContentRequest, Part};

    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("Hello")]));

    assert_eq!(req.model, "gemini-2.5-flash");
    assert_eq!(req.contents.len(), 1);
}

#[test]
fn gemini_function_calling_parts() {
    use abp_shim_gemini::Part;

    let call = Part::function_call("search", json!({"q": "rust"}));
    let resp = Part::function_response("search", json!({"results": []}));

    match call {
        Part::FunctionCall { name, .. } => assert_eq!(name, "search"),
        _ => panic!("expected FunctionCall"),
    }
    match resp {
        Part::FunctionResponse { name, .. } => assert_eq!(name, "search"),
        _ => panic!("expected FunctionResponse"),
    }
}

#[test]
fn gemini_multimodal_inline_data() {
    use abp_shim_gemini::{Content, GenerateContentRequest, Part};

    let req = GenerateContentRequest::new("gemini-2.5-flash").add_content(Content::user(vec![
        Part::text("What's in this image?"),
        Part::inline_data("image/png", "base64data"),
    ]));

    assert_eq!(req.contents[0].parts.len(), 2);
}

#[test]
fn gemini_safety_settings() {
    use abp_shim_gemini::{
        Content, GenerateContentRequest, HarmBlockThreshold, HarmCategory, Part, SafetySetting,
    };

    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("test")]))
        .safety_settings(vec![SafetySetting {
            category: HarmCategory::HarmCategoryHarassment,
            threshold: HarmBlockThreshold::BlockMediumAndAbove,
        }]);

    assert!(req.safety_settings.is_some());
    assert_eq!(req.safety_settings.as_ref().unwrap().len(), 1);
}

#[test]
fn gemini_generation_config() {
    use abp_shim_gemini::{Content, GenerateContentRequest, GenerationConfig, Part};

    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("test")]))
        .generation_config(GenerationConfig {
            max_output_tokens: Some(1024),
            temperature: Some(0.7),
            top_p: Some(0.9),
            top_k: Some(40),
            stop_sequences: Some(vec!["END".into()]),
            response_mime_type: None,
            response_schema: None,
        });

    let cfg = req.generation_config.unwrap();
    assert_eq!(cfg.max_output_tokens, Some(1024));
    assert_eq!(cfg.temperature, Some(0.7));
}

#[test]
fn gemini_to_dialect_request() {
    use abp_shim_gemini::{Content, GenerateContentRequest, Part, to_dialect_request};

    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("Hello")]));

    let dialect_req = to_dialect_request(&req);
    assert_eq!(dialect_req.model, "gemini-2.5-flash");
    assert_eq!(dialect_req.contents.len(), 1);
}

#[test]
fn gemini_from_dialect_response() {
    use abp_gemini_sdk::dialect::{GeminiContent, GeminiPart, GeminiResponse};
    use abp_shim_gemini::from_dialect_response;

    let dialect_resp = GeminiResponse {
        candidates: vec![abp_gemini_sdk::dialect::GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("Hi there".into())],
            },
            finish_reason: Some("STOP".into()),
            safety_ratings: None,
            citation_metadata: None,
        }],
        prompt_feedback: None,
        usage_metadata: None,
    };

    let resp = from_dialect_response(&dialect_resp);
    assert_eq!(resp.text(), Some("Hi there"));
}

#[test]
fn gemini_stream_event_handling() {
    use abp_gemini_sdk::dialect::{GeminiContent, GeminiPart, GeminiStreamChunk};
    use abp_shim_gemini::from_dialect_stream_chunk;

    let chunk = GeminiStreamChunk {
        candidates: vec![abp_gemini_sdk::dialect::GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("delta".into())],
            },
            finish_reason: None,
            safety_ratings: None,
            citation_metadata: None,
        }],
        usage_metadata: None,
    };

    let event = from_dialect_stream_chunk(&chunk);
    assert_eq!(event.text(), Some("delta"));
}

#[test]
fn gemini_system_instruction() {
    use abp_shim_gemini::{Content, GenerateContentRequest, Part};

    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("Hi")]))
        .system_instruction(Content::user(vec![Part::text("Be helpful")]));

    assert!(req.system_instruction.is_some());
}

#[test]
fn gemini_tool_declarations() {
    use abp_shim_gemini::{
        Content, FunctionCallingConfig, FunctionCallingMode, FunctionDeclaration,
        GenerateContentRequest, Part, ToolConfig, ToolDeclaration,
    };

    let req = GenerateContentRequest::new("gemini-2.5-flash")
        .add_content(Content::user(vec![Part::text("weather?")]))
        .tools(vec![ToolDeclaration {
            function_declarations: vec![FunctionDeclaration {
                name: "get_weather".into(),
                description: "Get weather".into(),
                parameters: json!({"type": "object"}),
            }],
        }])
        .tool_config(ToolConfig {
            function_calling_config: FunctionCallingConfig {
                mode: FunctionCallingMode::Auto,
                allowed_function_names: None,
            },
        });

    assert!(req.tools.is_some());
    assert!(req.tool_config.is_some());
}

#[test]
fn gemini_usage_to_ir() {
    use abp_shim_gemini::{UsageMetadata, usage_to_ir};

    let usage = UsageMetadata {
        prompt_token_count: 50,
        candidates_token_count: 30,
        total_token_count: 80,
    };

    let ir = usage_to_ir(&usage);
    assert_eq!(ir.input_tokens, 50);
    assert_eq!(ir.output_tokens, 30);
    assert_eq!(ir.total_tokens, 80);
}

#[test]
fn gemini_usage_roundtrip_ir() {
    use abp_shim_gemini::{UsageMetadata, usage_from_ir, usage_to_ir};

    let original = UsageMetadata {
        prompt_token_count: 100,
        candidates_token_count: 50,
        total_token_count: 150,
    };

    let ir = usage_to_ir(&original);
    let back = usage_from_ir(&ir);
    assert_eq!(back.prompt_token_count, original.prompt_token_count);
    assert_eq!(back.candidates_token_count, original.candidates_token_count);
}

// ═══════════════════════════════════════════════════════════════════════
//  4. Kimi shim surface (8 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn kimi_basic_chat_request() {
    use abp_shim_kimi::{KimiRequestBuilder, Message};

    let req = KimiRequestBuilder::new()
        .model("moonshot-v1-8k")
        .messages(vec![Message::user("What is AI?")])
        .temperature(0.3)
        .max_tokens(1024)
        .build();

    assert_eq!(req.model, "moonshot-v1-8k");
}

#[test]
fn kimi_tool_calling() {
    use abp_shim_kimi::{KimiFunctionDef, KimiRequestBuilder, Message};

    let tool = abp_kimi_sdk::dialect::KimiTool::Function {
        function: KimiFunctionDef {
            name: "search".into(),
            description: "Search the web".into(),
            parameters: json!({"type": "object"}),
        },
    };

    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("search for rust")])
        .tools(vec![tool])
        .build();

    assert!(req.tools.is_some());
}

#[test]
fn kimi_search_integration() {
    use abp_shim_kimi::{KimiRequestBuilder, Message};

    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("search query")])
        .use_search(true)
        .build();

    assert_eq!(req.use_search, Some(true));
}

#[test]
fn kimi_request_to_ir() {
    use abp_shim_kimi::{KimiRequestBuilder, Message, request_to_ir};

    let req = KimiRequestBuilder::new()
        .messages(vec![Message::user("Hello")])
        .build();

    let conv = request_to_ir(&req);
    assert!(!conv.is_empty());
}

#[test]
fn kimi_request_to_work_order() {
    use abp_shim_kimi::{KimiRequestBuilder, Message, request_to_work_order};

    let req = KimiRequestBuilder::new()
        .model("moonshot-v1-8k")
        .messages(vec![Message::user("Explain AI")])
        .build();

    let wo = request_to_work_order(&req);
    assert!(!wo.task.is_empty());
}

#[test]
fn kimi_receipt_to_response() {
    use abp_shim_kimi::{mock_receipt, receipt_to_response};

    let receipt = mock_receipt(vec![
        run_started_event(),
        assistant_message_event("AI is..."),
        run_completed_event(),
    ]);

    let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
    assert_eq!(resp.model, "moonshot-v1-8k");
}

#[test]
fn kimi_messages_roundtrip() {
    use abp_shim_kimi::{Message, ir_to_messages, messages_to_ir};

    let messages = vec![Message::user("Hello"), Message::assistant("Hi")];

    let conv = messages_to_ir(&messages);
    assert!(!conv.is_empty());

    let back = ir_to_messages(&conv);
    assert!(!back.is_empty());
}

#[test]
fn kimi_usage_conversion() {
    use abp_shim_kimi::ir_usage_to_usage;

    let usage = ir_usage_to_usage(&sample_ir_usage());
    assert_eq!(usage.prompt_tokens, 100);
    assert_eq!(usage.completion_tokens, 50);
    assert_eq!(usage.total_tokens, 150);
}

// ═══════════════════════════════════════════════════════════════════════
//  5. Codex shim surface (8 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn codex_request_builder() {
    use abp_shim_codex::{CodexRequestBuilder, codex_message};

    let req = CodexRequestBuilder::new()
        .model("codex-mini-latest")
        .input(vec![codex_message("user", "Write a function")])
        .max_output_tokens(2048)
        .temperature(0.0)
        .build();

    assert_eq!(req.model, "codex-mini-latest");
}

#[test]
fn codex_message_helper() {
    use abp_codex_sdk::dialect::CodexInputItem;
    use abp_shim_codex::codex_message;

    let msg = codex_message("user", "Hello");
    let CodexInputItem::Message { role, content } = msg;
    assert_eq!(role, "user");
    assert_eq!(content, "Hello");
}

#[test]
fn codex_request_to_ir() {
    use abp_shim_codex::{CodexRequestBuilder, codex_message, request_to_ir};

    let req = CodexRequestBuilder::new()
        .input(vec![codex_message("user", "code a parser")])
        .build();

    let conv = request_to_ir(&req);
    assert!(!conv.is_empty());
}

#[test]
fn codex_request_to_work_order() {
    use abp_shim_codex::{CodexRequestBuilder, codex_message, request_to_work_order};

    let req = CodexRequestBuilder::new()
        .model("codex-mini-latest")
        .input(vec![codex_message("user", "Write a sort function")])
        .build();

    let wo = request_to_work_order(&req);
    assert!(!wo.task.is_empty());
    assert_eq!(wo.config.model.as_deref(), Some("codex-mini-latest"));
}

#[test]
fn codex_receipt_to_response() {
    use abp_shim_codex::{mock_receipt, receipt_to_response};

    let receipt = mock_receipt(vec![
        run_started_event(),
        assistant_message_event("fn sort(v: &mut Vec<i32>) {}"),
        run_completed_event(),
    ]);

    let resp = receipt_to_response(&receipt, "codex-mini-latest");
    assert_eq!(resp.model, "codex-mini-latest");
    assert!(!resp.output.is_empty());
}

#[test]
fn codex_events_to_stream() {
    use abp_shim_codex::events_to_stream_events;

    let events = vec![assistant_message_event("code output")];
    let stream = events_to_stream_events(&events, "codex-mini-latest");
    assert!(!stream.is_empty());
}

#[test]
fn codex_ir_to_response_items() {
    use abp_shim_codex::ir_to_response_items;

    let conv =
        IrConversation::from_messages(vec![IrMessage::text(IrRole::Assistant, "fn main() {}")]);

    let items = ir_to_response_items(&conv);
    assert!(!items.is_empty());
}

#[test]
fn codex_usage_conversion() {
    use abp_shim_codex::ir_usage_to_usage;

    let usage = ir_usage_to_usage(&sample_ir_usage());
    assert_eq!(usage.input_tokens, 100);
    assert_eq!(usage.output_tokens, 50);
    assert_eq!(usage.total_tokens, 150);
}

// ═══════════════════════════════════════════════════════════════════════
//  6. Copilot shim surface (8 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn copilot_message_constructors() {
    use abp_shim_copilot::Message;

    let sys = Message::system("You are a coding assistant");
    let usr = Message::user("Help me");
    let ast = Message::assistant("Sure!");

    assert_eq!(sys.role, "system");
    assert_eq!(usr.role, "user");
    assert_eq!(ast.role, "assistant");
    assert_eq!(usr.content, "Help me");
}

#[test]
fn copilot_request_builder() {
    use abp_shim_copilot::{CopilotRequestBuilder, Message};

    let req = CopilotRequestBuilder::new()
        .model("gpt-4o")
        .messages(vec![
            Message::system("Be a coding assistant"),
            Message::user("What is Rust?"),
        ])
        .build();

    assert_eq!(req.model, "gpt-4o");
    assert_eq!(req.messages.len(), 2);
}

#[test]
fn copilot_request_to_ir() {
    use abp_shim_copilot::{CopilotRequestBuilder, Message, request_to_ir};

    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user("Hello")])
        .build();

    let conv = request_to_ir(&req);
    assert!(!conv.is_empty());
}

#[test]
fn copilot_request_to_work_order() {
    use abp_shim_copilot::{CopilotRequestBuilder, Message, request_to_work_order};

    let req = CopilotRequestBuilder::new()
        .model("gpt-4o")
        .messages(vec![Message::user("Explain async")])
        .build();

    let wo = request_to_work_order(&req);
    assert!(!wo.task.is_empty());
}

#[test]
fn copilot_receipt_to_response() {
    use abp_shim_copilot::{mock_receipt, receipt_to_response};

    let receipt = mock_receipt(vec![
        run_started_event(),
        assistant_message_event("Async is..."),
        run_completed_event(),
    ]);

    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.message, "Async is...");
}

#[test]
fn copilot_events_to_stream() {
    use abp_shim_copilot::events_to_stream_events;

    let events = vec![assistant_message_event("streaming text")];
    let stream = events_to_stream_events(&events, "gpt-4o");
    // First event is CopilotReferences, then text, then Done
    assert!(stream.len() >= 3);
}

#[test]
fn copilot_messages_roundtrip() {
    use abp_shim_copilot::{Message, ir_to_messages, messages_to_ir};

    let messages = vec![Message::user("Hello"), Message::assistant("Hi there")];

    let conv = messages_to_ir(&messages);
    assert!(!conv.is_empty());

    let back = ir_to_messages(&conv);
    assert!(!back.is_empty());
}

#[test]
fn copilot_usage_tuple() {
    use abp_shim_copilot::ir_usage_to_tuple;

    let (input, output, total) = ir_usage_to_tuple(&sample_ir_usage());
    assert_eq!(input, 100);
    assert_eq!(output, 50);
    assert_eq!(total, 150);
}

// ═══════════════════════════════════════════════════════════════════════
//  7. Cross-SDK compatibility matrix (12 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cross_sdk_openai_to_claude_mapper_exists() {
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Claude);
    assert!(mapper.is_some(), "OpenAi→Claude mapper should exist");
}

#[test]
fn cross_sdk_claude_to_openai_mapper_exists() {
    let mapper = default_ir_mapper(Dialect::Claude, Dialect::OpenAi);
    assert!(mapper.is_some(), "Claude→OpenAi mapper should exist");
}

#[test]
fn cross_sdk_openai_to_gemini_mapper_exists() {
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Gemini);
    assert!(mapper.is_some(), "OpenAi→Gemini mapper should exist");
}

#[test]
fn cross_sdk_gemini_to_openai_mapper_exists() {
    let mapper = default_ir_mapper(Dialect::Gemini, Dialect::OpenAi);
    assert!(mapper.is_some(), "Gemini→OpenAi mapper should exist");
}

#[test]
fn cross_sdk_claude_to_gemini_mapper_exists() {
    let mapper = default_ir_mapper(Dialect::Claude, Dialect::Gemini);
    assert!(mapper.is_some(), "Claude→Gemini mapper should exist");
}

#[test]
fn cross_sdk_gemini_to_claude_mapper_exists() {
    let mapper = default_ir_mapper(Dialect::Gemini, Dialect::Claude);
    assert!(mapper.is_some(), "Gemini→Claude mapper should exist");
}

#[test]
fn cross_sdk_all_identity_mappers_exist() {
    for &dialect in Dialect::all() {
        let mapper = default_ir_mapper(dialect, dialect);
        assert!(
            mapper.is_some(),
            "Identity mapper for {:?} should exist",
            dialect
        );
    }
}

#[test]
fn cross_sdk_supported_pairs_cover_all_identity() {
    let pairs = supported_ir_pairs();
    for &dialect in Dialect::all() {
        assert!(
            pairs.contains(&(dialect, dialect)),
            "Identity pair ({:?}, {:?}) missing from supported pairs",
            dialect,
            dialect
        );
    }
}

#[test]
fn cross_sdk_supported_pairs_minimum_count() {
    let pairs = supported_ir_pairs();
    // At minimum: 6 identity + 6 cross-dialect = 12
    assert!(
        pairs.len() >= 12,
        "Expected at least 12 supported pairs, got {}",
        pairs.len()
    );
}

#[test]
fn cross_sdk_openai_claude_ir_request_mapping() {
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
    let conv =
        IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "Hello from OpenAI")]);

    let mapped = mapper.map_request(Dialect::OpenAi, Dialect::Claude, &conv);
    assert!(mapped.is_ok());
    let result = mapped.unwrap();
    assert!(!result.is_empty());
}

#[test]
fn cross_sdk_openai_gemini_ir_request_mapping() {
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Gemini).unwrap();
    let conv =
        IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "Hello from OpenAI")]);

    let mapped = mapper.map_request(Dialect::OpenAi, Dialect::Gemini, &conv);
    assert!(mapped.is_ok());
}

#[test]
fn cross_sdk_identity_preserves_conversation() {
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::OpenAi).unwrap();
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "System prompt"),
        IrMessage::text(IrRole::User, "User message"),
        IrMessage::text(IrRole::Assistant, "Assistant reply"),
    ]);

    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::OpenAi, &conv)
        .unwrap();
    assert_eq!(mapped.messages.len(), conv.messages.len());
}
