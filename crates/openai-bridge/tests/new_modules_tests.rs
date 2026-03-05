#![allow(clippy::all)]
#![allow(clippy::needless_update)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for new modules: streaming, function_calling, embeddings, and translate enhancements.

use openai_bridge::embeddings::*;
use openai_bridge::function_calling::*;
use openai_bridge::openai_types::*;
use openai_bridge::streaming::*;

// ═══════════════════════════════════════════════════════════════════════
// SSE streaming tests
// ═══════════════════════════════════════════════════════════════════════

fn sse_chunk(id: &str, content: Option<&str>, finish: Option<&str>) -> String {
    let delta = if let Some(c) = content {
        format!(r#"{{"content":"{}"}}"#, c)
    } else {
        "{}".to_string()
    };
    let finish_json = match finish {
        Some(f) => format!(r#""{}""#, f),
        None => "null".to_string(),
    };
    format!(
        r#"data: {{"id":"{}","object":"chat.completion.chunk","created":0,"model":"gpt-4o","choices":[{{"index":0,"delta":{},"finish_reason":{}}}]}}"#,
        id, delta, finish_json
    )
}

#[test]
fn sse_single_text_chunk() {
    let body = format!("{}\n\n", sse_chunk("c1", Some("Hi"), None));
    let events = parse_sse_stream(&body);
    assert_eq!(events.len(), 1);
    match &events[0] {
        SseEvent::Chunk(c) => assert_eq!(c.choices[0].delta.content.as_deref(), Some("Hi")),
        _ => panic!("expected Chunk"),
    }
}

#[test]
fn sse_done_terminates() {
    let body = format!("{}\n\ndata: [DONE]\n\n", sse_chunk("c1", Some("A"), None));
    let events = parse_sse_stream(&body);
    assert_eq!(events.len(), 2);
    assert!(matches!(events[0], SseEvent::Chunk(_)));
    assert_eq!(events[1], SseEvent::Done);
}

#[test]
fn sse_error_stream_invalid_json() {
    let body = "data: {broken\n\ndata: [DONE]\n\n";
    let events = parse_sse_stream(body);
    assert_eq!(events.len(), 2);
    assert!(matches!(events[0], SseEvent::ParseError { .. }));
    assert_eq!(events[1], SseEvent::Done);
}

#[test]
fn sse_partial_line_buffering() {
    let mut parser = SseParser::new();
    let line = sse_chunk("c1", Some("X"), None);
    // Feed half
    let half = line.len() / 2;
    assert!(parser.feed(&line[..half]).is_empty());
    // Feed rest + newline
    let events = parser.feed(&format!("{}\n", &line[half..]));
    assert_eq!(events.len(), 1);
}

#[test]
fn sse_comments_and_non_data_ignored() {
    let body = ": comment\nevent: keep-alive\nid: 42\nretry: 1000\ndata: [DONE]\n\n";
    let events = parse_sse_stream(body);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0], SseEvent::Done);
}

#[test]
fn sse_crlf_handling() {
    let body = format!(
        "{}\r\n\r\ndata: [DONE]\r\n\r\n",
        sse_chunk("c1", Some("Y"), None)
    );
    let events = parse_sse_stream(&body);
    assert_eq!(events.len(), 2);
}

#[test]
fn sse_many_chunks_then_done() {
    let mut body = String::new();
    for i in 0..10 {
        body.push_str(&format!(
            "{}\n\n",
            sse_chunk("c1", Some(&format!("{}", i)), None)
        ));
    }
    body.push_str("data: [DONE]\n\n");
    let events = parse_sse_stream(&body);
    assert_eq!(events.len(), 11);
    assert_eq!(events[10], SseEvent::Done);
}

#[test]
fn sse_empty_data_lines_skipped() {
    let body = "data: \n\ndata:\n\ndata: [DONE]\n\n";
    let events = parse_sse_stream(body);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0], SseEvent::Done);
}

#[test]
fn sse_flush_without_newline() {
    let mut parser = SseParser::new();
    parser.feed("data: [DONE]");
    assert!(!parser.is_done());
    let events = parser.flush();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0], SseEvent::Done);
}

#[test]
fn sse_interleaved_comments() {
    let body = format!(
        ": keepalive\n{}\n\n: ping\n{}\n\ndata: [DONE]\n\n",
        sse_chunk("c1", Some("A"), None),
        sse_chunk("c1", Some("B"), None),
    );
    let events = parse_sse_stream(&body);
    assert_eq!(events.len(), 3);
}

// ═══════════════════════════════════════════════════════════════════════
// Function calling tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn tool_choice_auto_round_trip() {
    let tc = ToolChoice::auto();
    let json = serde_json::to_string(&tc).unwrap();
    let back: ToolChoice = serde_json::from_str(&json).unwrap();
    assert_eq!(tc, back);
}

#[test]
fn tool_choice_none_round_trip() {
    let tc = ToolChoice::none();
    let json = serde_json::to_string(&tc).unwrap();
    assert_eq!(json, r#""none""#);
}

#[test]
fn tool_choice_required_round_trip() {
    let tc = ToolChoice::required();
    let json = serde_json::to_string(&tc).unwrap();
    assert_eq!(json, r#""required""#);
}

#[test]
fn tool_choice_named_round_trip() {
    let tc = ToolChoice::named("my_func");
    let json = serde_json::to_string(&tc).unwrap();
    let back: ToolChoice = serde_json::from_str(&json).unwrap();
    assert_eq!(tc, back);
    assert!(json.contains("my_func"));
}

#[test]
fn parallel_assembler_two_calls() {
    let mut asm = ParallelToolCallAssembler::new();
    asm.feed(&StreamToolCall {
        index: 0,
        id: Some("c1".into()),
        call_type: Some("function".into()),
        function: Some(StreamFunctionCall {
            name: Some("read".into()),
            arguments: Some(r#"{"p":"#.into()),
        }),
    });
    asm.feed(&StreamToolCall {
        index: 1,
        id: Some("c2".into()),
        call_type: Some("function".into()),
        function: Some(StreamFunctionCall {
            name: Some("write".into()),
            arguments: Some(r#"{"d":"#.into()),
        }),
    });
    asm.feed(&StreamToolCall {
        index: 0,
        id: None,
        call_type: None,
        function: Some(StreamFunctionCall {
            name: None,
            arguments: Some(r#""a.txt"}"#.into()),
        }),
    });
    asm.feed(&StreamToolCall {
        index: 1,
        id: None,
        call_type: None,
        function: Some(StreamFunctionCall {
            name: None,
            arguments: Some(r#""ok"}"#.into()),
        }),
    });
    let calls = asm.finish();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].function.arguments, r#"{"p":"a.txt"}"#);
    assert_eq!(calls[1].function.arguments, r#"{"d":"ok"}"#);
}

#[test]
fn parallel_assembler_empty_finish() {
    let asm = ParallelToolCallAssembler::new();
    assert!(asm.is_empty());
    assert_eq!(asm.finish().len(), 0);
}

#[test]
fn parallel_assembler_feed_all_convenience() {
    let mut asm = ParallelToolCallAssembler::new();
    asm.feed_all(&[StreamToolCall {
        index: 0,
        id: Some("x".into()),
        call_type: Some("function".into()),
        function: Some(StreamFunctionCall {
            name: Some("f1".into()),
            arguments: Some("{}".into()),
        }),
    }]);
    assert_eq!(asm.len(), 1);
}

#[test]
fn parallel_assembler_sparse_indices() {
    let mut asm = ParallelToolCallAssembler::new();
    asm.feed(&StreamToolCall {
        index: 2,
        id: Some("c3".into()),
        call_type: Some("function".into()),
        function: Some(StreamFunctionCall {
            name: Some("fn3".into()),
            arguments: Some("{}".into()),
        }),
    });
    assert_eq!(asm.len(), 3); // indices 0,1,2 created
    let calls = asm.finish();
    assert_eq!(calls.len(), 1); // only index 2 has an id
    assert_eq!(calls[0].id, "c3");
}

#[test]
fn tool_definition_builder_works() {
    let td = ToolDefinition::function("search", "Search", serde_json::json!({}));
    assert_eq!(td.tool_type, "function");
    assert_eq!(td.function.name, "search");
}

#[test]
fn tool_call_builder_works() {
    let tc = ToolCall::function("id1", "fn1", r#"{"a":1}"#);
    assert_eq!(tc.call_type, "function");
    assert_eq!(tc.function.name, "fn1");
}

#[test]
fn strict_function_def_serde() {
    let sfd = StrictFunctionDefinition {
        name: "calc".into(),
        description: "Calculator".into(),
        parameters: serde_json::json!({"type":"object"}),
        strict: Some(true),
    };
    let json = serde_json::to_string(&sfd).unwrap();
    let back: StrictFunctionDefinition = serde_json::from_str(&json).unwrap();
    assert_eq!(sfd, back);
}

#[test]
fn strict_tool_def_serde() {
    let std = StrictToolDefinition {
        tool_type: "function".into(),
        function: StrictFunctionDefinition {
            name: "f".into(),
            description: "d".into(),
            parameters: serde_json::json!({}),
            strict: None,
        },
    };
    let json = serde_json::to_string(&std).unwrap();
    let back: StrictToolDefinition = serde_json::from_str(&json).unwrap();
    assert_eq!(std, back);
}

// ═══════════════════════════════════════════════════════════════════════
// Embedding type tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn embedding_request_single_serde() {
    let req = EmbeddingRequest::single("text-embedding-3-small", "hello world");
    let json = serde_json::to_string(&req).unwrap();
    let back: EmbeddingRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(req, back);
}

#[test]
fn embedding_request_batch_serde() {
    let req = EmbeddingRequest::batch("model", vec!["a".into(), "b".into(), "c".into()]);
    let json = serde_json::to_string(&req).unwrap();
    let back: EmbeddingRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(req, back);
    match &back.input {
        EmbeddingInput::Batch(v) => assert_eq!(v.len(), 3),
        _ => panic!("expected Batch"),
    }
}

#[test]
fn embedding_request_with_options() {
    let req = EmbeddingRequest::single("m", "text")
        .with_encoding_format(EncodingFormat::Base64)
        .with_dimensions(512)
        .with_user("u1");
    assert_eq!(req.encoding_format, Some(EncodingFormat::Base64));
    assert_eq!(req.dimensions, Some(512));
    assert_eq!(req.user.as_deref(), Some("u1"));
}

#[test]
fn embedding_request_skips_nones() {
    let req = EmbeddingRequest::single("m", "t");
    let json = serde_json::to_string(&req).unwrap();
    assert!(!json.contains("encoding_format"));
    assert!(!json.contains("dimensions"));
    assert!(!json.contains("user"));
}

#[test]
fn embedding_response_serde() {
    let resp = EmbeddingResponse {
        object: "list".into(),
        data: vec![EmbeddingObject {
            object: "embedding".into(),
            embedding: vec![0.1, 0.2, 0.3],
            index: 0,
        }],
        model: "text-embedding-3-small".into(),
        usage: EmbeddingUsage {
            prompt_tokens: 5,
            total_tokens: 5,
        },
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: EmbeddingResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(resp, back);
}

#[test]
fn embedding_response_multiple_objects() {
    let resp = EmbeddingResponse {
        object: "list".into(),
        data: vec![
            EmbeddingObject {
                object: "embedding".into(),
                embedding: vec![1.0],
                index: 0,
            },
            EmbeddingObject {
                object: "embedding".into(),
                embedding: vec![2.0],
                index: 1,
            },
        ],
        model: "m".into(),
        usage: EmbeddingUsage {
            prompt_tokens: 10,
            total_tokens: 10,
        },
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: EmbeddingResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(back.data.len(), 2);
}

#[test]
fn embedding_response_empty_data() {
    let resp = EmbeddingResponse {
        object: "list".into(),
        data: vec![],
        model: "m".into(),
        usage: EmbeddingUsage::default(),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: EmbeddingResponse = serde_json::from_str(&json).unwrap();
    assert!(back.data.is_empty());
}

#[test]
fn embedding_object_empty_vec() {
    let obj = EmbeddingObject {
        object: "embedding".into(),
        embedding: vec![],
        index: 0,
    };
    let json = serde_json::to_string(&obj).unwrap();
    let back: EmbeddingObject = serde_json::from_str(&json).unwrap();
    assert!(back.embedding.is_empty());
}

#[test]
fn embedding_usage_defaults() {
    let u = EmbeddingUsage::default();
    assert_eq!(u.prompt_tokens, 0);
    assert_eq!(u.total_tokens, 0);
}

#[test]
fn encoding_format_values() {
    assert_eq!(
        serde_json::to_string(&EncodingFormat::Float).unwrap(),
        r#""float""#
    );
    assert_eq!(
        serde_json::to_string(&EncodingFormat::Base64).unwrap(),
        r#""base64""#
    );
}

#[test]
fn embedding_input_single_is_string() {
    let input = EmbeddingInput::Single("test".into());
    let json = serde_json::to_string(&input).unwrap();
    assert_eq!(json, r#""test""#);
}

#[test]
fn embedding_input_batch_is_array() {
    let input = EmbeddingInput::Batch(vec!["a".into(), "b".into()]);
    let json = serde_json::to_string(&input).unwrap();
    assert_eq!(json, r#"["a","b"]"#);
}

#[test]
fn parse_realistic_embedding_response() {
    let json = r#"{
        "object": "list",
        "data": [
            {"object": "embedding", "embedding": [0.0023, -0.0094, 0.0156], "index": 0}
        ],
        "model": "text-embedding-3-small",
        "usage": {"prompt_tokens": 8, "total_tokens": 8}
    }"#;
    let resp: EmbeddingResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.data.len(), 1);
    assert_eq!(resp.data[0].embedding.len(), 3);
}

// ═══════════════════════════════════════════════════════════════════════
// Translation enhancement tests (feature-gated)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn translate_chunk_text_delta() {
    let chunk = ChatCompletionChunk {
        id: "c1".into(),
        object: "chat.completion.chunk".into(),
        created: 0,
        model: "gpt-4o".into(),
        choices: vec![StreamChoice {
            index: 0,
            delta: StreamDelta {
                role: None,
                content: Some("hello".into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
    };
    assert_eq!(
        openai_bridge::translate::chunk_text_delta(&chunk),
        Some("hello".into())
    );
}

#[test]
fn translate_chunk_text_delta_none_when_no_content() {
    let chunk = ChatCompletionChunk {
        id: "c1".into(),
        object: "chat.completion.chunk".into(),
        created: 0,
        model: "gpt-4o".into(),
        choices: vec![StreamChoice {
            index: 0,
            delta: StreamDelta::default(),
            finish_reason: None,
        }],
    };
    assert!(openai_bridge::translate::chunk_text_delta(&chunk).is_none());
}

#[test]
fn translate_chunk_text_delta_empty_choices() {
    let chunk = ChatCompletionChunk {
        id: "c1".into(),
        object: "chat.completion.chunk".into(),
        created: 0,
        model: "gpt-4o".into(),
        choices: vec![],
    };
    assert!(openai_bridge::translate::chunk_text_delta(&chunk).is_none());
}

#[test]
fn translate_chunk_finish_reason() {
    let chunk = ChatCompletionChunk {
        id: "c1".into(),
        object: "chat.completion.chunk".into(),
        created: 0,
        model: "gpt-4o".into(),
        choices: vec![StreamChoice {
            index: 0,
            delta: StreamDelta::default(),
            finish_reason: Some("stop".into()),
        }],
    };
    assert_eq!(
        openai_bridge::translate::chunk_finish_reason(&chunk),
        Some("stop".into())
    );
}

#[test]
fn translate_chunk_finish_reason_none() {
    let chunk = ChatCompletionChunk {
        id: "c1".into(),
        object: "chat.completion.chunk".into(),
        created: 0,
        model: "gpt-4o".into(),
        choices: vec![StreamChoice {
            index: 0,
            delta: StreamDelta::default(),
            finish_reason: None,
        }],
    };
    assert!(openai_bridge::translate::chunk_finish_reason(&chunk).is_none());
}

#[test]
fn translate_stream_to_ir_message_text_only() {
    let msg = openai_bridge::translate::stream_to_ir_message("Hello world", &[]);
    assert_eq!(msg.role, abp_core::ir::IrRole::Assistant);
    assert_eq!(msg.content.len(), 1);
    match &msg.content[0] {
        abp_core::ir::IrContentBlock::Text { text } => assert_eq!(text, "Hello world"),
        _ => panic!("expected Text"),
    }
}

#[test]
fn translate_stream_to_ir_message_with_tool_calls() {
    let tool_calls = vec![ToolCall {
        id: "call_1".into(),
        call_type: "function".into(),
        function: FunctionCall {
            name: "search".into(),
            arguments: r#"{"q":"rust"}"#.into(),
        },
    }];
    let msg = openai_bridge::translate::stream_to_ir_message("", &tool_calls);
    assert_eq!(msg.content.len(), 1); // no text, one tool use
    match &msg.content[0] {
        abp_core::ir::IrContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "call_1");
            assert_eq!(name, "search");
            assert_eq!(*input, serde_json::json!({"q":"rust"}));
        }
        _ => panic!("expected ToolUse"),
    }
}

#[test]
fn translate_stream_to_ir_message_text_and_tool_calls() {
    let tool_calls = vec![ToolCall::function("c1", "fn1", "{}")];
    let msg = openai_bridge::translate::stream_to_ir_message("Thinking...", &tool_calls);
    assert_eq!(msg.content.len(), 2);
}

#[test]
fn translate_stream_to_ir_message_empty() {
    let msg = openai_bridge::translate::stream_to_ir_message("", &[]);
    assert!(msg.content.is_empty());
}

#[test]
fn translate_stream_to_ir_message_invalid_args() {
    let tool_calls = vec![ToolCall::function("c1", "fn1", "not json")];
    let msg = openai_bridge::translate::stream_to_ir_message("", &tool_calls);
    match &msg.content[0] {
        abp_core::ir::IrContentBlock::ToolUse { input, .. } => {
            assert_eq!(*input, serde_json::Value::Null);
        }
        _ => panic!("expected ToolUse"),
    }
}

#[test]
fn translate_embedding_usage_to_ir() {
    let usage = openai_bridge::embeddings::EmbeddingUsage {
        prompt_tokens: 42,
        total_tokens: 42,
    };
    let ir = openai_bridge::translate::embedding_usage_to_ir(&usage);
    assert_eq!(ir.input_tokens, 42);
    assert_eq!(ir.output_tokens, 0);
    assert_eq!(ir.total_tokens, 42);
}

#[test]
fn translate_tool_choice_to_value_auto() {
    let tc = ToolChoice::auto();
    let val = openai_bridge::translate::tool_choice_to_value(&tc);
    assert_eq!(val, serde_json::json!("auto"));
}

#[test]
fn translate_tool_choice_to_value_named() {
    let tc = ToolChoice::named("my_fn");
    let val = openai_bridge::translate::tool_choice_to_value(&tc);
    assert!(val.is_object());
}

#[test]
fn translate_tool_choice_from_value_roundtrip() {
    let tc = ToolChoice::required();
    let val = openai_bridge::translate::tool_choice_to_value(&tc);
    let back = openai_bridge::translate::tool_choice_from_value(&val).unwrap();
    assert_eq!(tc, back);
}

#[test]
fn translate_tool_choice_from_value_invalid() {
    let val = serde_json::json!(12345);
    let result = openai_bridge::translate::tool_choice_from_value(&val);
    assert!(result.is_err());
}
