// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for Claude ↔ abp-dialect IR translation.

use claude_bridge::claude_types::*;
use claude_bridge::ir_translate::*;

use abp_dialect::ir::{
    IrContentBlock, IrGenerationConfig, IrMessage, IrRequest, IrResponse, IrRole, IrStopReason,
    IrStreamEvent, IrToolDefinition, IrUsage,
};
use serde_json::json;
use std::collections::BTreeMap;

// ── Helper builders ─────────────────────────────────────────────────────

fn simple_request() -> MessagesRequest {
    MessagesRequest {
        model: "claude-sonnet-4-20250514".into(),
        messages: vec![Message {
            role: Role::User,
            content: MessageContent::Text("Hello".into()),
        }],
        max_tokens: 1024,
        system: None,
        tools: None,
        metadata: None,
        stream: None,
        stop_sequences: None,
        temperature: None,
        top_p: None,
        top_k: None,
        tool_choice: None,
        thinking: None,
    }
}

fn simple_response() -> MessagesResponse {
    MessagesResponse {
        id: "msg_01".into(),
        response_type: "message".into(),
        role: "assistant".into(),
        content: vec![ContentBlock::Text {
            text: "Hi there!".into(),
        }],
        model: "claude-sonnet-4-20250514".into(),
        stop_reason: Some("end_turn".into()),
        stop_sequence: None,
        usage: Usage {
            input_tokens: 10,
            output_tokens: 20,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        },
    }
}

// ── Request to IR ───────────────────────────────────────────────────────

#[test]
fn request_simple_text() {
    let req = simple_request();
    let ir = claude_request_to_ir(&req);
    assert_eq!(ir.model, Some("claude-sonnet-4-20250514".into()));
    assert_eq!(ir.messages.len(), 1);
    assert_eq!(ir.messages[0].role, IrRole::User);
    assert!(ir.system_prompt.is_none());
}

#[test]
fn request_system_prompt_text() {
    let mut req = simple_request();
    req.system = Some(SystemMessage::Text("You are helpful.".into()));
    let ir = claude_request_to_ir(&req);
    assert_eq!(ir.system_prompt, Some("You are helpful.".into()));
}

#[test]
fn request_system_prompt_blocks() {
    let mut req = simple_request();
    req.system = Some(SystemMessage::Blocks(vec![
        SystemBlock::Text {
            text: "Rule 1.".into(),
            cache_control: None,
        },
        SystemBlock::Text {
            text: "Rule 2.".into(),
            cache_control: None,
        },
    ]));
    let ir = claude_request_to_ir(&req);
    assert_eq!(ir.system_prompt, Some("Rule 1.\nRule 2.".into()));
}

#[test]
fn request_max_tokens_mapped() {
    let mut req = simple_request();
    req.max_tokens = 8192;
    let ir = claude_request_to_ir(&req);
    assert_eq!(ir.config.max_tokens, Some(8192));
}

#[test]
fn request_temperature() {
    let mut req = simple_request();
    req.temperature = Some(0.7);
    let ir = claude_request_to_ir(&req);
    assert_eq!(ir.config.temperature, Some(0.7));
}

#[test]
fn request_top_p_and_top_k() {
    let mut req = simple_request();
    req.top_p = Some(0.9);
    req.top_k = Some(40);
    let ir = claude_request_to_ir(&req);
    assert_eq!(ir.config.top_p, Some(0.9));
    assert_eq!(ir.config.top_k, Some(40));
}

#[test]
fn request_stop_sequences() {
    let mut req = simple_request();
    req.stop_sequences = Some(vec!["STOP".into(), "END".into()]);
    let ir = claude_request_to_ir(&req);
    assert_eq!(ir.config.stop_sequences, vec!["STOP", "END"]);
}

#[test]
fn request_tools() {
    let mut req = simple_request();
    req.tools = Some(vec![ToolDefinition {
        name: "read_file".into(),
        description: "Read a file".into(),
        input_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
    }]);
    let ir = claude_request_to_ir(&req);
    assert_eq!(ir.tools.len(), 1);
    assert_eq!(ir.tools[0].name, "read_file");
    assert_eq!(ir.tools[0].description, "Read a file");
}

#[test]
fn request_tool_choice_in_metadata() {
    let mut req = simple_request();
    req.tool_choice = Some(ToolChoice::Auto {});
    let ir = claude_request_to_ir(&req);
    assert!(ir.metadata.contains_key("tool_choice"));
}

#[test]
fn request_thinking_in_metadata() {
    let mut req = simple_request();
    req.thinking = Some(ThinkingConfig {
        thinking_type: "enabled".into(),
        budget_tokens: 10000,
    });
    let ir = claude_request_to_ir(&req);
    assert!(ir.metadata.contains_key("thinking"));
}

#[test]
fn request_stream_flag() {
    let mut req = simple_request();
    req.stream = Some(true);
    let ir = claude_request_to_ir(&req);
    assert_eq!(
        ir.metadata.get("stream"),
        Some(&serde_json::Value::Bool(true))
    );
}

#[test]
fn request_multiblock_message() {
    let mut req = simple_request();
    req.messages = vec![Message {
        role: Role::User,
        content: MessageContent::Blocks(vec![
            ContentBlock::Text {
                text: "Look at this:".into(),
            },
            ContentBlock::Image {
                source: ImageSource::Base64 {
                    media_type: "image/png".into(),
                    data: "iVBOR...".into(),
                },
            },
        ]),
    }];
    let ir = claude_request_to_ir(&req);
    assert_eq!(ir.messages[0].content.len(), 2);
    assert!(matches!(
        &ir.messages[0].content[1],
        IrContentBlock::Image { .. }
    ));
}

#[test]
fn request_tool_result_message() {
    let mut req = simple_request();
    req.messages = vec![Message {
        role: Role::User,
        content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
            tool_use_id: "toolu_01".into(),
            content: Some("file contents here".into()),
            is_error: None,
        }]),
    }];
    let ir = claude_request_to_ir(&req);
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolResult {
            tool_call_id,
            content,
            is_error,
        } => {
            assert_eq!(tool_call_id, "toolu_01");
            assert_eq!(content.len(), 1);
            assert!(!is_error);
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn request_tool_use_block() {
    let mut req = simple_request();
    req.messages = vec![Message {
        role: Role::Assistant,
        content: MessageContent::Blocks(vec![ContentBlock::ToolUse {
            id: "toolu_01".into(),
            name: "read_file".into(),
            input: json!({"path": "/tmp/test.txt"}),
        }]),
    }];
    let ir = claude_request_to_ir(&req);
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolCall { id, name, input } => {
            assert_eq!(id, "toolu_01");
            assert_eq!(name, "read_file");
            assert_eq!(input, &json!({"path": "/tmp/test.txt"}));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn request_thinking_block() {
    let mut req = simple_request();
    req.messages = vec![Message {
        role: Role::Assistant,
        content: MessageContent::Blocks(vec![ContentBlock::Thinking {
            thinking: "Let me analyze...".into(),
            signature: Some("sig_abc".into()),
        }]),
    }];
    let ir = claude_request_to_ir(&req);
    match &ir.messages[0].content[0] {
        IrContentBlock::Thinking { text } => {
            assert_eq!(text, "Let me analyze...");
        }
        other => panic!("expected Thinking, got {other:?}"),
    }
}

// ── IR to Request ───────────────────────────────────────────────────────

#[test]
fn ir_to_request_basic() {
    let ir = IrRequest {
        model: Some("claude-sonnet-4-20250514".into()),
        system_prompt: Some("Be helpful".into()),
        messages: vec![IrMessage::text(IrRole::User, "Hello")],
        tools: Vec::new(),
        config: IrGenerationConfig {
            max_tokens: Some(2048),
            temperature: Some(0.5),
            ..Default::default()
        },
        metadata: BTreeMap::new(),
    };
    let req = ir_to_claude_request(&ir);
    assert_eq!(req.model, "claude-sonnet-4-20250514");
    assert_eq!(req.max_tokens, 2048);
    assert_eq!(req.temperature, Some(0.5));
    assert_eq!(req.system, Some(SystemMessage::Text("Be helpful".into())));
    assert_eq!(req.messages.len(), 1);
}

#[test]
fn ir_to_request_default_max_tokens() {
    let ir = IrRequest::new(vec![IrMessage::text(IrRole::User, "Hi")]);
    let req = ir_to_claude_request(&ir);
    assert_eq!(req.max_tokens, 4096);
}

#[test]
fn ir_to_request_default_model() {
    let ir = IrRequest::new(vec![IrMessage::text(IrRole::User, "Hi")]);
    let req = ir_to_claude_request(&ir);
    assert_eq!(req.model, "claude-sonnet-4-20250514");
}

#[test]
fn ir_to_request_tools() {
    let ir = IrRequest {
        model: Some("claude-sonnet-4-20250514".into()),
        system_prompt: None,
        messages: vec![IrMessage::text(IrRole::User, "list files")],
        tools: vec![IrToolDefinition {
            name: "ls".into(),
            description: "List directory".into(),
            parameters: json!({"type": "object"}),
        }],
        config: IrGenerationConfig::default(),
        metadata: BTreeMap::new(),
    };
    let req = ir_to_claude_request(&ir);
    assert_eq!(req.tools.as_ref().unwrap().len(), 1);
    assert_eq!(req.tools.as_ref().unwrap()[0].name, "ls");
}

#[test]
fn ir_to_request_stop_sequences() {
    let ir = IrRequest {
        model: Some("claude-sonnet-4-20250514".into()),
        system_prompt: None,
        messages: vec![IrMessage::text(IrRole::User, "Hi")],
        tools: Vec::new(),
        config: IrGenerationConfig {
            stop_sequences: vec!["STOP".into()],
            ..Default::default()
        },
        metadata: BTreeMap::new(),
    };
    let req = ir_to_claude_request(&ir);
    assert_eq!(req.stop_sequences, Some(vec!["STOP".into()]));
}

#[test]
fn ir_to_request_thinking_roundtrip() {
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "thinking".into(),
        json!({"type": "enabled", "budget_tokens": 5000}),
    );
    let ir = IrRequest {
        model: Some("claude-sonnet-4-20250514".into()),
        system_prompt: None,
        messages: vec![IrMessage::text(IrRole::User, "think hard")],
        tools: Vec::new(),
        config: IrGenerationConfig::default(),
        metadata,
    };
    let req = ir_to_claude_request(&ir);
    let th = req.thinking.unwrap();
    assert_eq!(th.thinking_type, "enabled");
    assert_eq!(th.budget_tokens, 5000);
}

// ── Response to IR ──────────────────────────────────────────────────────

#[test]
fn response_simple_text() {
    let resp = simple_response();
    let ir = claude_response_to_ir(&resp);
    assert_eq!(ir.id, Some("msg_01".into()));
    assert_eq!(ir.model, Some("claude-sonnet-4-20250514".into()));
    assert_eq!(ir.content.len(), 1);
    assert_eq!(ir.content[0].as_text(), Some("Hi there!"));
    assert_eq!(ir.stop_reason, Some(IrStopReason::EndTurn));
}

#[test]
fn response_usage() {
    let resp = simple_response();
    let ir = claude_response_to_ir(&resp);
    let u = ir.usage.unwrap();
    assert_eq!(u.input_tokens, 10);
    assert_eq!(u.output_tokens, 20);
    assert_eq!(u.total_tokens, 30);
}

#[test]
fn response_with_cache_usage() {
    let mut resp = simple_response();
    resp.usage.cache_creation_input_tokens = Some(100);
    resp.usage.cache_read_input_tokens = Some(50);
    let ir = claude_response_to_ir(&resp);
    let u = ir.usage.unwrap();
    assert_eq!(u.cache_write_tokens, 100);
    assert_eq!(u.cache_read_tokens, 50);
}

#[test]
fn response_tool_use() {
    let mut resp = simple_response();
    resp.content = vec![ContentBlock::ToolUse {
        id: "toolu_01".into(),
        name: "read_file".into(),
        input: json!({"path": "src/main.rs"}),
    }];
    resp.stop_reason = Some("tool_use".into());
    let ir = claude_response_to_ir(&resp);
    assert_eq!(ir.stop_reason, Some(IrStopReason::ToolUse));
    match &ir.content[0] {
        IrContentBlock::ToolCall { name, .. } => assert_eq!(name, "read_file"),
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn response_max_tokens_stop() {
    let mut resp = simple_response();
    resp.stop_reason = Some("max_tokens".into());
    let ir = claude_response_to_ir(&resp);
    assert_eq!(ir.stop_reason, Some(IrStopReason::MaxTokens));
}

#[test]
fn response_stop_sequence() {
    let mut resp = simple_response();
    resp.stop_reason = Some("stop_sequence".into());
    let ir = claude_response_to_ir(&resp);
    assert_eq!(ir.stop_reason, Some(IrStopReason::StopSequence));
}

#[test]
fn response_unknown_stop_reason() {
    let mut resp = simple_response();
    resp.stop_reason = Some("custom_reason".into());
    let ir = claude_response_to_ir(&resp);
    assert_eq!(
        ir.stop_reason,
        Some(IrStopReason::Other("custom_reason".into()))
    );
}

#[test]
fn response_no_stop_reason() {
    let mut resp = simple_response();
    resp.stop_reason = None;
    let ir = claude_response_to_ir(&resp);
    assert!(ir.stop_reason.is_none());
}

#[test]
fn response_thinking_block() {
    let mut resp = simple_response();
    resp.content = vec![
        ContentBlock::Thinking {
            thinking: "Analyzing...".into(),
            signature: Some("sig".into()),
        },
        ContentBlock::Text {
            text: "Here is my answer.".into(),
        },
    ];
    let ir = claude_response_to_ir(&resp);
    assert_eq!(ir.content.len(), 2);
    match &ir.content[0] {
        IrContentBlock::Thinking { text } => assert_eq!(text, "Analyzing..."),
        other => panic!("expected Thinking, got {other:?}"),
    }
}

// ── IR to Response ──────────────────────────────────────────────────────

#[test]
fn ir_to_response_basic() {
    let ir = IrResponse {
        id: Some("msg_02".into()),
        model: Some("claude-sonnet-4-20250514".into()),
        content: vec![IrContentBlock::Text {
            text: "Hello!".into(),
        }],
        stop_reason: Some(IrStopReason::EndTurn),
        usage: Some(IrUsage::from_io(5, 10)),
        metadata: BTreeMap::new(),
    };
    let resp = ir_to_claude_response(&ir);
    assert_eq!(resp.id, "msg_02");
    assert_eq!(resp.model, "claude-sonnet-4-20250514");
    assert_eq!(resp.response_type, "message");
    assert_eq!(resp.role, "assistant");
    assert_eq!(resp.stop_reason, Some("end_turn".into()));
    assert_eq!(resp.usage.input_tokens, 5);
    assert_eq!(resp.usage.output_tokens, 10);
}

#[test]
fn ir_to_response_tool_call() {
    let ir = IrResponse::new(vec![IrContentBlock::ToolCall {
        id: "toolu_01".into(),
        name: "exec".into(),
        input: json!({"cmd": "ls"}),
    }])
    .with_stop_reason(IrStopReason::ToolUse);
    let resp = ir_to_claude_response(&ir);
    assert_eq!(resp.stop_reason, Some("tool_use".into()));
    match &resp.content[0] {
        ContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "toolu_01");
            assert_eq!(name, "exec");
            assert_eq!(input, &json!({"cmd": "ls"}));
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn ir_to_response_no_usage() {
    let ir = IrResponse::new(vec![IrContentBlock::Text { text: "ok".into() }]);
    let resp = ir_to_claude_response(&ir);
    assert_eq!(resp.usage.input_tokens, 0);
    assert_eq!(resp.usage.output_tokens, 0);
}

#[test]
fn ir_to_response_cache_usage() {
    let usage = IrUsage {
        input_tokens: 100,
        output_tokens: 50,
        total_tokens: 150,
        cache_read_tokens: 30,
        cache_write_tokens: 20,
    };
    let ir = IrResponse::new(vec![IrContentBlock::Text { text: "ok".into() }]).with_usage(usage);
    let resp = ir_to_claude_response(&ir);
    assert_eq!(resp.usage.cache_read_input_tokens, Some(30));
    assert_eq!(resp.usage.cache_creation_input_tokens, Some(20));
}

// ── Stream events ───────────────────────────────────────────────────────

#[test]
fn stream_message_start() {
    let event = StreamEvent::MessageStart {
        message: MessagesResponse {
            id: "msg_stream".into(),
            response_type: "message".into(),
            role: "assistant".into(),
            content: vec![],
            model: "claude-sonnet-4-20250514".into(),
            stop_reason: None,
            stop_sequence: None,
            usage: Usage {
                input_tokens: 15,
                output_tokens: 0,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
        },
    };
    let ir_events = claude_stream_to_ir(&event);
    assert_eq!(ir_events.len(), 2); // StreamStart + Usage
    match &ir_events[0] {
        IrStreamEvent::StreamStart { id, model } => {
            assert_eq!(id, &Some("msg_stream".into()));
            assert_eq!(model, &Some("claude-sonnet-4-20250514".into()));
        }
        other => panic!("expected StreamStart, got {other:?}"),
    }
    match &ir_events[1] {
        IrStreamEvent::Usage { usage } => {
            assert_eq!(usage.input_tokens, 15);
        }
        other => panic!("expected Usage, got {other:?}"),
    }
}

#[test]
fn stream_content_block_start_text() {
    let event = StreamEvent::ContentBlockStart {
        index: 0,
        content_block: ContentBlock::Text { text: "".into() },
    };
    let ir_events = claude_stream_to_ir(&event);
    assert_eq!(ir_events.len(), 1);
    match &ir_events[0] {
        IrStreamEvent::ContentBlockStart { index, block } => {
            assert_eq!(*index, 0);
            assert!(matches!(block, IrContentBlock::Text { text } if text.is_empty()));
        }
        other => panic!("expected ContentBlockStart, got {other:?}"),
    }
}

#[test]
fn stream_text_delta() {
    let event = StreamEvent::ContentBlockDelta {
        index: 0,
        delta: StreamDelta::TextDelta {
            text: "Hello ".into(),
        },
    };
    let ir_events = claude_stream_to_ir(&event);
    assert_eq!(ir_events.len(), 1);
    match &ir_events[0] {
        IrStreamEvent::TextDelta { index, text } => {
            assert_eq!(*index, 0);
            assert_eq!(text, "Hello ");
        }
        other => panic!("expected TextDelta, got {other:?}"),
    }
}

#[test]
fn stream_tool_call_delta() {
    let event = StreamEvent::ContentBlockDelta {
        index: 1,
        delta: StreamDelta::InputJsonDelta {
            partial_json: r#"{"pa"#.into(),
        },
    };
    let ir_events = claude_stream_to_ir(&event);
    assert_eq!(ir_events.len(), 1);
    match &ir_events[0] {
        IrStreamEvent::ToolCallDelta {
            index,
            arguments_delta,
        } => {
            assert_eq!(*index, 1);
            assert_eq!(arguments_delta, r#"{"pa"#);
        }
        other => panic!("expected ToolCallDelta, got {other:?}"),
    }
}

#[test]
fn stream_thinking_delta() {
    let event = StreamEvent::ContentBlockDelta {
        index: 0,
        delta: StreamDelta::ThinkingDelta {
            thinking: "Let me ".into(),
        },
    };
    let ir_events = claude_stream_to_ir(&event);
    assert_eq!(ir_events.len(), 1);
    match &ir_events[0] {
        IrStreamEvent::ThinkingDelta { index, text } => {
            assert_eq!(*index, 0);
            assert_eq!(text, "Let me ");
        }
        other => panic!("expected ThinkingDelta, got {other:?}"),
    }
}

#[test]
fn stream_signature_delta_ignored() {
    let event = StreamEvent::ContentBlockDelta {
        index: 0,
        delta: StreamDelta::SignatureDelta {
            signature: "sig_part".into(),
        },
    };
    let ir_events = claude_stream_to_ir(&event);
    assert!(ir_events.is_empty());
}

#[test]
fn stream_content_block_stop() {
    let event = StreamEvent::ContentBlockStop { index: 2 };
    let ir_events = claude_stream_to_ir(&event);
    assert_eq!(ir_events.len(), 1);
    match &ir_events[0] {
        IrStreamEvent::ContentBlockStop { index } => assert_eq!(*index, 2),
        other => panic!("expected ContentBlockStop, got {other:?}"),
    }
}

#[test]
fn stream_message_delta_with_stop() {
    let event = StreamEvent::MessageDelta {
        delta: claude_bridge::claude_types::MessageDelta {
            stop_reason: Some("end_turn".into()),
            stop_sequence: None,
        },
        usage: Some(Usage {
            input_tokens: 0,
            output_tokens: 42,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        }),
    };
    let ir_events = claude_stream_to_ir(&event);
    assert_eq!(ir_events.len(), 2); // Usage + StreamEnd
    match &ir_events[0] {
        IrStreamEvent::Usage { usage } => assert_eq!(usage.output_tokens, 42),
        other => panic!("expected Usage, got {other:?}"),
    }
    match &ir_events[1] {
        IrStreamEvent::StreamEnd { stop_reason } => {
            assert_eq!(*stop_reason, Some(IrStopReason::EndTurn));
        }
        other => panic!("expected StreamEnd, got {other:?}"),
    }
}

#[test]
fn stream_message_stop_empty() {
    let event = StreamEvent::MessageStop {};
    let ir_events = claude_stream_to_ir(&event);
    assert!(ir_events.is_empty());
}

#[test]
fn stream_ping_empty() {
    let event = StreamEvent::Ping {};
    let ir_events = claude_stream_to_ir(&event);
    assert!(ir_events.is_empty());
}

#[test]
fn stream_error() {
    let event = StreamEvent::Error {
        error: ApiError {
            error_type: "overloaded_error".into(),
            message: "API is overloaded".into(),
        },
    };
    let ir_events = claude_stream_to_ir(&event);
    assert_eq!(ir_events.len(), 1);
    match &ir_events[0] {
        IrStreamEvent::Error { code, message } => {
            assert_eq!(code, "overloaded_error");
            assert_eq!(message, "API is overloaded");
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

// ── Roundtrip tests ─────────────────────────────────────────────────────

#[test]
fn request_roundtrip() {
    let orig = simple_request();
    let ir = claude_request_to_ir(&orig);
    let back = ir_to_claude_request(&ir);
    assert_eq!(back.model, orig.model);
    assert_eq!(back.max_tokens, orig.max_tokens);
    assert_eq!(back.messages.len(), orig.messages.len());
}

#[test]
fn response_roundtrip() {
    let orig = simple_response();
    let ir = claude_response_to_ir(&orig);
    let back = ir_to_claude_response(&ir);
    assert_eq!(back.id, orig.id);
    assert_eq!(back.model, orig.model);
    assert_eq!(back.stop_reason, orig.stop_reason);
    assert_eq!(back.usage.input_tokens, orig.usage.input_tokens);
    assert_eq!(back.usage.output_tokens, orig.usage.output_tokens);
}

#[test]
fn request_with_tools_roundtrip() {
    let mut req = simple_request();
    req.tools = Some(vec![ToolDefinition {
        name: "search".into(),
        description: "Search the web".into(),
        input_schema: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
    }]);
    req.temperature = Some(0.3);
    req.top_p = Some(0.95);
    let ir = claude_request_to_ir(&req);
    let back = ir_to_claude_request(&ir);
    let tools = back.tools.unwrap();
    assert_eq!(tools[0].name, "search");
    assert_eq!(back.temperature, Some(0.3));
    assert_eq!(back.top_p, Some(0.95));
}

#[test]
fn image_url_becomes_text_in_ir() {
    let block = ContentBlock::Image {
        source: ImageSource::Url {
            url: "https://example.com/img.png".into(),
        },
    };
    let mut req = simple_request();
    req.messages = vec![Message {
        role: Role::User,
        content: MessageContent::Blocks(vec![block]),
    }];
    let ir = claude_request_to_ir(&req);
    match &ir.messages[0].content[0] {
        IrContentBlock::Text { text } => {
            assert!(text.contains("https://example.com/img.png"));
        }
        other => panic!("expected Text (URL placeholder), got {other:?}"),
    }
}

#[test]
fn tool_result_error_flag() {
    let mut req = simple_request();
    req.messages = vec![Message {
        role: Role::User,
        content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
            tool_use_id: "toolu_err".into(),
            content: Some("something went wrong".into()),
            is_error: Some(true),
        }]),
    }];
    let ir = claude_request_to_ir(&req);
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolResult { is_error, .. } => assert!(is_error),
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn content_filter_stop_reason_roundtrip() {
    let ir = IrResponse::text("blocked").with_stop_reason(IrStopReason::ContentFilter);
    let resp = ir_to_claude_response(&ir);
    assert_eq!(resp.stop_reason, Some("content_filter".into()));
}

#[test]
fn stream_content_block_start_tool_use() {
    let event = StreamEvent::ContentBlockStart {
        index: 1,
        content_block: ContentBlock::ToolUse {
            id: "toolu_01".into(),
            name: "bash".into(),
            input: json!({}),
        },
    };
    let ir_events = claude_stream_to_ir(&event);
    assert_eq!(ir_events.len(), 1);
    match &ir_events[0] {
        IrStreamEvent::ContentBlockStart { index, block } => {
            assert_eq!(*index, 1);
            assert!(matches!(block, IrContentBlock::ToolCall { name, .. } if name == "bash"));
        }
        other => panic!("expected ContentBlockStart, got {other:?}"),
    }
}
