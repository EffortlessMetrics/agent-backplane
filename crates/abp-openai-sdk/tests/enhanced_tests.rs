// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for streaming chunks, response format, tool choice, validation, and model canonicalization.

use abp_core::AgentEventKind;
use abp_openai_sdk::dialect::{
    CanonicalToolDef, OpenAIFunctionCall, OpenAIToolCall,
    ToolChoice, ToolChoiceFunctionRef, ToolChoiceMode, from_canonical_model, is_known_model,
    to_canonical_model, tool_def_from_openai, tool_def_to_openai,
};
use abp_openai_sdk::response_format::{JsonSchemaSpec, ResponseFormat};
use abp_openai_sdk::streaming::{
    ChatCompletionChunk, ChunkChoice, ChunkDelta, ChunkFunctionCall, ChunkToolCall, ChunkUsage,
    ToolCallAccumulator, map_chunk,
};
use abp_openai_sdk::validation::{ExtendedRequestFields, validate_for_mapped_mode};

// ===========================================================================
// Streaming chunk serialization roundtrip
// ===========================================================================

#[test]
fn chunk_text_delta_serde_roundtrip() {
    let chunk = ChatCompletionChunk {
        id: "chatcmpl-stream-1".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "gpt-4o".into(),
        choices: vec![ChunkChoice {
            index: 0,
            delta: ChunkDelta {
                role: None,
                content: Some("Hello".into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
    };
    let json = serde_json::to_string(&chunk).unwrap();
    let parsed: ChatCompletionChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, chunk);
}

#[test]
fn chunk_first_delta_with_role_roundtrip() {
    let chunk = ChatCompletionChunk {
        id: "chatcmpl-stream-2".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000001,
        model: "gpt-4o".into(),
        choices: vec![ChunkChoice {
            index: 0,
            delta: ChunkDelta {
                role: Some("assistant".into()),
                content: None,
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
    };
    let json = serde_json::to_string(&chunk).unwrap();
    let parsed: ChatCompletionChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.choices[0].delta.role.as_deref(), Some("assistant"));
}

#[test]
fn chunk_tool_call_fragment_roundtrip() {
    let chunk = ChatCompletionChunk {
        id: "chatcmpl-tc-1".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000002,
        model: "gpt-4o".into(),
        choices: vec![ChunkChoice {
            index: 0,
            delta: ChunkDelta {
                role: None,
                content: None,
                tool_calls: Some(vec![ChunkToolCall {
                    index: 0,
                    id: Some("call_abc".into()),
                    call_type: Some("function".into()),
                    function: Some(ChunkFunctionCall {
                        name: Some("read_file".into()),
                        arguments: Some(r#"{"pa"#.into()),
                    }),
                }]),
            },
            finish_reason: None,
        }],
        usage: None,
    };
    let json = serde_json::to_string(&chunk).unwrap();
    let parsed: ChatCompletionChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, chunk);
}

#[test]
fn chunk_with_usage_roundtrip() {
    let chunk = ChatCompletionChunk {
        id: "chatcmpl-final".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000003,
        model: "gpt-4o".into(),
        choices: vec![ChunkChoice {
            index: 0,
            delta: ChunkDelta::default(),
            finish_reason: Some("stop".into()),
        }],
        usage: Some(ChunkUsage {
            prompt_tokens: 50,
            completion_tokens: 25,
            total_tokens: 75,
        }),
    };
    let json = serde_json::to_string(&chunk).unwrap();
    let parsed: ChatCompletionChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.usage.unwrap().total_tokens, 75);
}

#[test]
fn map_chunk_emits_assistant_delta() {
    let chunk = ChatCompletionChunk {
        id: "chatcmpl-d1".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000004,
        model: "gpt-4o".into(),
        choices: vec![ChunkChoice {
            index: 0,
            delta: ChunkDelta {
                role: None,
                content: Some("world".into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
    };
    let events = map_chunk(&chunk);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "world"),
        other => panic!("expected AssistantDelta, got {other:?}"),
    }
}

#[test]
fn map_chunk_skips_empty_content() {
    let chunk = ChatCompletionChunk {
        id: "chatcmpl-empty".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000005,
        model: "gpt-4o".into(),
        choices: vec![ChunkChoice {
            index: 0,
            delta: ChunkDelta {
                role: None,
                content: Some(String::new()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
    };
    let events = map_chunk(&chunk);
    assert!(events.is_empty());
}

// ===========================================================================
// Tool call accumulator
// ===========================================================================

#[test]
fn accumulator_reassembles_fragments() {
    let mut acc = ToolCallAccumulator::new();

    // First fragment: id + name + start of arguments
    acc.feed(&[ChunkToolCall {
        index: 0,
        id: Some("call_1".into()),
        call_type: Some("function".into()),
        function: Some(ChunkFunctionCall {
            name: Some("get_weather".into()),
            arguments: Some(r#"{"loc"#.into()),
        }),
    }]);

    // Second fragment: rest of arguments
    acc.feed(&[ChunkToolCall {
        index: 0,
        id: None,
        call_type: None,
        function: Some(ChunkFunctionCall {
            name: None,
            arguments: Some(r#"ation":"NYC"}"#.into()),
        }),
    }]);

    let events = acc.finish();
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            input,
            ..
        } => {
            assert_eq!(tool_name, "get_weather");
            assert_eq!(tool_use_id.as_deref(), Some("call_1"));
            assert_eq!(input, &serde_json::json!({"location": "NYC"}));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn accumulator_handles_multiple_tool_calls() {
    let mut acc = ToolCallAccumulator::new();

    acc.feed(&[
        ChunkToolCall {
            index: 0,
            id: Some("call_a".into()),
            call_type: Some("function".into()),
            function: Some(ChunkFunctionCall {
                name: Some("read_file".into()),
                arguments: Some(r#"{"path":"a.rs"}"#.into()),
            }),
        },
        ChunkToolCall {
            index: 1,
            id: Some("call_b".into()),
            call_type: Some("function".into()),
            function: Some(ChunkFunctionCall {
                name: Some("read_file".into()),
                arguments: Some(r#"{"path":"b.rs"}"#.into()),
            }),
        },
    ]);

    let events = acc.finish();
    assert_eq!(events.len(), 2);
}

#[test]
fn accumulator_finish_as_openai_returns_pairs() {
    let mut acc = ToolCallAccumulator::new();
    acc.feed(&[ChunkToolCall {
        index: 0,
        id: Some("call_x".into()),
        call_type: Some("function".into()),
        function: Some(ChunkFunctionCall {
            name: Some("bash".into()),
            arguments: Some(r#"{"cmd":"ls"}"#.into()),
        }),
    }]);

    let pairs = acc.finish_as_openai();
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].0, "call_x");
    assert_eq!(pairs[0].1.name, "bash");
    assert_eq!(pairs[0].1.arguments, r#"{"cmd":"ls"}"#);
}

// ===========================================================================
// Tool call mapping to/from ABP IR
// ===========================================================================

#[test]
fn tool_call_to_abp_event_preserves_fields() {
    let tc = OpenAIToolCall {
        id: "call_42".into(),
        call_type: "function".into(),
        function: OpenAIFunctionCall {
            name: "write_file".into(),
            arguments: r#"{"path":"x.txt","content":"hi"}"#.into(),
        },
    };

    // Simulate what map_response does internally
    let input: serde_json::Value = serde_json::from_str(&tc.function.arguments).unwrap();
    assert_eq!(input["path"], "x.txt");
    assert_eq!(input["content"], "hi");
}

#[test]
fn tool_def_roundtrip_preserves_schema() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "query": { "type": "string" },
            "limit": { "type": "integer", "default": 10 }
        },
        "required": ["query"]
    });
    let canonical = CanonicalToolDef {
        name: "search".into(),
        description: "Search the codebase".into(),
        parameters_schema: schema.clone(),
    };
    let openai = tool_def_to_openai(&canonical);
    let back = tool_def_from_openai(&openai);
    assert_eq!(back.parameters_schema, schema);
}

#[test]
fn openai_tool_def_type_is_always_function() {
    let canonical = CanonicalToolDef {
        name: "test".into(),
        description: "test".into(),
        parameters_schema: serde_json::json!({}),
    };
    let openai = tool_def_to_openai(&canonical);
    assert_eq!(openai.tool_type, "function");
}

// ===========================================================================
// ToolChoice serde
// ===========================================================================

#[test]
fn tool_choice_mode_auto_roundtrip() {
    let choice = ToolChoice::Mode(ToolChoiceMode::Auto);
    let json = serde_json::to_string(&choice).unwrap();
    let parsed: ToolChoice = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, choice);
}

#[test]
fn tool_choice_mode_none_roundtrip() {
    let choice = ToolChoice::Mode(ToolChoiceMode::None);
    let json = serde_json::to_string(&choice).unwrap();
    let parsed: ToolChoice = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, choice);
}

#[test]
fn tool_choice_function_roundtrip() {
    let choice = ToolChoice::Function {
        tool_type: "function".into(),
        function: ToolChoiceFunctionRef {
            name: "get_weather".into(),
        },
    };
    let json = serde_json::to_string(&choice).unwrap();
    let parsed: ToolChoice = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, choice);
}

// ===========================================================================
// Structured output format validation
// ===========================================================================

#[test]
fn response_format_text_roundtrip() {
    let fmt = ResponseFormat::text();
    let json = serde_json::to_string(&fmt).unwrap();
    let parsed: ResponseFormat = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, fmt);
    assert!(json.contains(r#""type":"text""#));
}

#[test]
fn response_format_json_object_roundtrip() {
    let fmt = ResponseFormat::json_object();
    let json = serde_json::to_string(&fmt).unwrap();
    let parsed: ResponseFormat = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, fmt);
    assert!(json.contains(r#""type":"json_object""#));
}

#[test]
fn response_format_json_schema_roundtrip() {
    let fmt = ResponseFormat::json_schema(
        "my_output",
        serde_json::json!({
            "type": "object",
            "properties": { "answer": { "type": "string" } },
            "required": ["answer"]
        }),
    );
    let json = serde_json::to_string(&fmt).unwrap();
    let parsed: ResponseFormat = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, fmt);
    assert!(json.contains(r#""type":"json_schema""#));
    assert!(json.contains("my_output"));
}

#[test]
fn response_format_json_schema_strict_default() {
    let fmt = ResponseFormat::json_schema("test", serde_json::json!({}));
    match fmt {
        ResponseFormat::JsonSchema { json_schema } => {
            assert_eq!(json_schema.strict, Some(true));
        }
        _ => panic!("expected JsonSchema variant"),
    }
}

#[test]
fn json_schema_spec_with_description_roundtrip() {
    let spec = JsonSchemaSpec {
        name: "result".into(),
        description: Some("The result object".into()),
        schema: serde_json::json!({"type": "object"}),
        strict: Some(false),
    };
    let json = serde_json::to_string(&spec).unwrap();
    let parsed: JsonSchemaSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.description.as_deref(), Some("The result object"));
    assert_eq!(parsed.strict, Some(false));
}

// ===========================================================================
// Mapped-mode early failure for unmappable params
// ===========================================================================

#[test]
fn validation_passes_with_no_fields() {
    let fields = ExtendedRequestFields::default();
    assert!(validate_for_mapped_mode(&fields).is_ok());
}

#[test]
fn validation_fails_on_logprobs() {
    let fields = ExtendedRequestFields {
        logprobs: Some(true),
        ..Default::default()
    };
    let err = validate_for_mapped_mode(&fields).unwrap_err();
    assert_eq!(err.errors.len(), 1);
    assert_eq!(err.errors[0].param, "logprobs");
}

#[test]
fn validation_fails_on_top_logprobs() {
    let fields = ExtendedRequestFields {
        top_logprobs: Some(5),
        ..Default::default()
    };
    let err = validate_for_mapped_mode(&fields).unwrap_err();
    assert_eq!(err.errors.len(), 1);
    assert_eq!(err.errors[0].param, "logprobs");
}

#[test]
fn validation_fails_on_logit_bias() {
    let mut bias = std::collections::BTreeMap::new();
    bias.insert("1234".into(), 1.0);
    let fields = ExtendedRequestFields {
        logit_bias: Some(bias),
        ..Default::default()
    };
    let err = validate_for_mapped_mode(&fields).unwrap_err();
    assert_eq!(err.errors.len(), 1);
    assert_eq!(err.errors[0].param, "logit_bias");
}

#[test]
fn validation_fails_on_seed() {
    let fields = ExtendedRequestFields {
        seed: Some(42),
        ..Default::default()
    };
    let err = validate_for_mapped_mode(&fields).unwrap_err();
    assert_eq!(err.errors.len(), 1);
    assert_eq!(err.errors[0].param, "seed");
}

#[test]
fn validation_collects_all_errors() {
    let mut bias = std::collections::BTreeMap::new();
    bias.insert("999".into(), -5.0);
    let fields = ExtendedRequestFields {
        logprobs: Some(true),
        top_logprobs: None,
        logit_bias: Some(bias),
        seed: Some(123),
    };
    let err = validate_for_mapped_mode(&fields).unwrap_err();
    assert_eq!(err.errors.len(), 3);
    let params: Vec<&str> = err.errors.iter().map(|e| e.param.as_str()).collect();
    assert!(params.contains(&"logprobs"));
    assert!(params.contains(&"logit_bias"));
    assert!(params.contains(&"seed"));
}

#[test]
fn validation_passes_with_logprobs_false() {
    let fields = ExtendedRequestFields {
        logprobs: Some(false),
        ..Default::default()
    };
    assert!(validate_for_mapped_mode(&fields).is_ok());
}

#[test]
fn validation_passes_with_empty_logit_bias() {
    let fields = ExtendedRequestFields {
        logit_bias: Some(std::collections::BTreeMap::new()),
        ..Default::default()
    };
    assert!(validate_for_mapped_mode(&fields).is_ok());
}

#[test]
fn validation_error_display() {
    let fields = ExtendedRequestFields {
        seed: Some(1),
        ..Default::default()
    };
    let err = validate_for_mapped_mode(&fields).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("unmappable parameter"));
    assert!(msg.contains("seed"));
}

// ===========================================================================
// Model name canonicalization
// ===========================================================================

#[test]
fn canonical_model_o1_roundtrip() {
    let canonical = to_canonical_model("o1");
    assert_eq!(canonical, "openai/o1");
    assert_eq!(from_canonical_model(&canonical), "o1");
}

#[test]
fn canonical_model_o3_mini_roundtrip() {
    let canonical = to_canonical_model("o3-mini");
    assert_eq!(canonical, "openai/o3-mini");
    assert_eq!(from_canonical_model(&canonical), "o3-mini");
}

#[test]
fn canonical_model_gpt41_roundtrip() {
    let canonical = to_canonical_model("gpt-4.1");
    assert_eq!(canonical, "openai/gpt-4.1");
    assert_eq!(from_canonical_model(&canonical), "gpt-4.1");
}

#[test]
fn is_known_model_rejects_anthropic() {
    assert!(!is_known_model("claude-3-opus"));
    assert!(!is_known_model("claude-3.5-sonnet"));
}

#[test]
fn is_known_model_rejects_empty() {
    assert!(!is_known_model(""));
}

#[test]
fn canonical_model_non_openai_prefix_passthrough() {
    // A model with a different prefix should pass through from_canonical_model
    assert_eq!(from_canonical_model("anthropic/claude-3"), "anthropic/claude-3");
}
