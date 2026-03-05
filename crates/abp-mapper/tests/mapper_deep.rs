#![allow(clippy::all)]
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
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]

//! Comprehensive tests for the mapper crate covering registry, identity mapping,
//! cross-dialect mapping (OpenAI↔Claude, OpenAI↔Gemini, Claude↔Gemini), lossy
//! detection, unsupported features, system messages, tool schemas, streaming
//! events, round trips, error taxonomy, and edge cases.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_core::{AgentEvent, AgentEventKind};
use abp_dialect::Dialect;
use abp_mapper::validation::{
    DefaultMappingValidator, MappingValidator, ValidationPipeline, ValidationSeverity,
};
use abp_mapper::{
    ClaudeGeminiIrMapper, ClaudeToOpenAiMapper, CodexClaudeIrMapper, DialectRequest,
    DialectResponse, GeminiToOpenAiMapper, IdentityMapper, IrIdentityMapper, IrMapper, MapError,
    Mapper, MappingError, OpenAiClaudeIrMapper, OpenAiCodexIrMapper, OpenAiGeminiIrMapper,
    OpenAiToClaudeMapper, OpenAiToGeminiMapper, default_ir_mapper, supported_ir_pairs,
};
use chrono::Utc;
use serde_json::{Value, json};

// ── Helpers ─────────────────────────────────────────────────────────────

fn simple_ir() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are helpful."),
        IrMessage::text(IrRole::User, "Hello"),
        IrMessage::text(IrRole::Assistant, "Hi there!"),
    ])
}

fn tool_call_ir() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Check the weather"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Let me check.".into(),
                },
                IrContentBlock::ToolUse {
                    id: "call_42".into(),
                    name: "get_weather".into(),
                    input: json!({"city": "NYC"}),
                },
            ],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "call_42".into(),
                content: vec![IrContentBlock::Text {
                    text: "72°F, sunny".into(),
                }],
                is_error: false,
            }],
        ),
        IrMessage::text(IrRole::Assistant, "It's 72°F and sunny in NYC."),
    ])
}

fn thinking_ir() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Solve 2+2"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "2+2 is 4".into(),
                },
                IrContentBlock::Text {
                    text: "The answer is 4.".into(),
                },
            ],
        ),
    ])
}

fn openai_chat_request(messages: Value) -> DialectRequest {
    DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": messages,
        }),
    }
}

fn claude_chat_request(messages: Value, system: Option<&str>) -> DialectRequest {
    let mut body = json!({
        "model": "claude-3-5-sonnet-20241022",
        "max_tokens": 1024,
        "messages": messages,
    });
    if let Some(sys) = system {
        body["system"] = json!(sys);
    }
    DialectRequest {
        dialect: Dialect::Claude,
        body,
    }
}

fn gemini_chat_request(contents: Value) -> DialectRequest {
    DialectRequest {
        dialect: Dialect::Gemini,
        body: json!({
            "model": "gemini-pro",
            "contents": contents,
        }),
    }
}

// =========================================================================
// 1. MapperRegistry: register, lookup, list dialect mappers
// =========================================================================

#[test]
fn registry_factory_resolves_all_identity_pairs() {
    for &d in Dialect::all() {
        let mapper = default_ir_mapper(d, d);
        assert!(mapper.is_some(), "no identity mapper for {d}");
    }
}

#[test]
fn registry_factory_resolves_openai_claude_pair() {
    assert!(default_ir_mapper(Dialect::OpenAi, Dialect::Claude).is_some());
    assert!(default_ir_mapper(Dialect::Claude, Dialect::OpenAi).is_some());
}

#[test]
fn registry_factory_resolves_openai_gemini_pair() {
    assert!(default_ir_mapper(Dialect::OpenAi, Dialect::Gemini).is_some());
    assert!(default_ir_mapper(Dialect::Gemini, Dialect::OpenAi).is_some());
}

#[test]
fn registry_factory_resolves_claude_gemini_pair() {
    assert!(default_ir_mapper(Dialect::Claude, Dialect::Gemini).is_some());
    assert!(default_ir_mapper(Dialect::Gemini, Dialect::Claude).is_some());
}

#[test]
fn registry_factory_returns_none_for_unknown_pairs() {
    // Codex↔Gemini has no direct mapper
    assert!(default_ir_mapper(Dialect::Codex, Dialect::Gemini).is_none());
    assert!(default_ir_mapper(Dialect::Gemini, Dialect::Codex).is_none());
}

#[test]
fn registry_supported_pairs_not_empty() {
    let pairs = supported_ir_pairs();
    assert!(!pairs.is_empty());
}

#[test]
fn registry_supported_pairs_covers_identity() {
    let pairs = supported_ir_pairs();
    for &d in Dialect::all() {
        assert!(pairs.contains(&(d, d)));
    }
}

#[test]
fn registry_supported_pairs_includes_cross_dialect() {
    let pairs = supported_ir_pairs();
    assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Claude)));
    assert!(pairs.contains(&(Dialect::Claude, Dialect::OpenAi)));
    assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Gemini)));
    assert!(pairs.contains(&(Dialect::Gemini, Dialect::OpenAi)));
}

#[test]
fn registry_mapper_supported_pairs_match_factory() {
    let mappers: Vec<Box<dyn IrMapper>> = vec![
        Box::new(OpenAiClaudeIrMapper),
        Box::new(OpenAiGeminiIrMapper),
        Box::new(ClaudeGeminiIrMapper),
        Box::new(OpenAiCodexIrMapper),
        Box::new(CodexClaudeIrMapper),
    ];
    for mapper in &mappers {
        for (from, to) in mapper.supported_pairs() {
            assert!(
                default_ir_mapper(from, to).is_some(),
                "factory missing ({from}, {to})"
            );
        }
    }
}

// =========================================================================
// 2. Identity mapping: same dialect → identity transform
// =========================================================================

#[test]
fn identity_json_mapper_preserves_request_body() {
    let mapper = IdentityMapper;
    let body = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: body.clone(),
    };
    assert_eq!(mapper.map_request(&req).unwrap(), body);
}

#[test]
fn identity_json_mapper_preserves_response_body() {
    let mapper = IdentityMapper;
    let body = json!({"id": "chatcmpl-1", "choices": []});
    let resp = mapper.map_response(&body).unwrap();
    assert_eq!(resp.body, body);
}

#[test]
fn identity_json_mapper_works_for_all_dialects() {
    let mapper = IdentityMapper;
    for &d in Dialect::all() {
        let req = DialectRequest {
            dialect: d,
            body: json!({"test": true}),
        };
        assert_eq!(mapper.map_request(&req).unwrap(), json!({"test": true}));
    }
}

#[test]
fn identity_ir_mapper_preserves_simple_conversation() {
    let mapper = IrIdentityMapper;
    let ir = simple_ir();
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::OpenAi, &ir)
        .unwrap();
    assert_eq!(ir, mapped);
}

#[test]
fn identity_ir_mapper_preserves_tool_calls() {
    let mapper = IrIdentityMapper;
    let ir = tool_call_ir();
    let mapped = mapper
        .map_request(Dialect::Claude, Dialect::Claude, &ir)
        .unwrap();
    assert_eq!(ir, mapped);
}

#[test]
fn identity_ir_mapper_supports_all_same_dialect_pairs() {
    let pairs = IrIdentityMapper.supported_pairs();
    for &d in Dialect::all() {
        assert!(pairs.contains(&(d, d)));
    }
}

#[test]
fn identity_ir_mapper_response_matches_request() {
    let mapper = IrIdentityMapper;
    let ir = simple_ir();
    let req_result = mapper
        .map_request(Dialect::OpenAi, Dialect::OpenAi, &ir)
        .unwrap();
    let resp_result = mapper
        .map_response(Dialect::OpenAi, Dialect::OpenAi, &ir)
        .unwrap();
    assert_eq!(req_result, resp_result);
}

// =========================================================================
// 3. OpenAI → Claude mapping
// =========================================================================

#[test]
fn openai_to_claude_basic_user_message() {
    let mapper = OpenAiToClaudeMapper;
    let req = openai_chat_request(json!([{"role": "user", "content": "Hello"}]));
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["messages"][0]["role"], "user");
    assert_eq!(result["messages"][0]["content"], "Hello");
}

#[test]
fn openai_to_claude_system_extracted() {
    let mapper = OpenAiToClaudeMapper;
    let req = openai_chat_request(json!([
        {"role": "system", "content": "Be concise."},
        {"role": "user", "content": "Hi"}
    ]));
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["system"], "Be concise.");
    assert_eq!(result["messages"].as_array().unwrap().len(), 1);
}

#[test]
fn openai_to_claude_multiple_system_joined() {
    let mapper = OpenAiToClaudeMapper;
    let req = openai_chat_request(json!([
        {"role": "system", "content": "Rule 1."},
        {"role": "system", "content": "Rule 2."},
        {"role": "user", "content": "Go"}
    ]));
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["system"], "Rule 1.\n\nRule 2.");
}

#[test]
fn openai_to_claude_model_passthrough() {
    let mapper = OpenAiToClaudeMapper;
    let req = openai_chat_request(json!([{"role": "user", "content": "hi"}]));
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["model"], "gpt-4");
}

#[test]
fn openai_to_claude_default_max_tokens() {
    let mapper = OpenAiToClaudeMapper;
    let req = openai_chat_request(json!([{"role": "user", "content": "hi"}]));
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["max_tokens"], 4096);
}

#[test]
fn openai_to_claude_explicit_max_tokens() {
    let mapper = OpenAiToClaudeMapper;
    let mut req = openai_chat_request(json!([{"role": "user", "content": "hi"}]));
    req.body["max_tokens"] = json!(2048);
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["max_tokens"], 2048);
}

#[test]
fn openai_to_claude_temperature_mapped() {
    let mapper = OpenAiToClaudeMapper;
    let mut req = openai_chat_request(json!([{"role": "user", "content": "hi"}]));
    req.body["temperature"] = json!(0.5);
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["temperature"], 0.5);
}

#[test]
fn openai_to_claude_tool_definitions() {
    let mapper = OpenAiToClaudeMapper;
    let mut req = openai_chat_request(json!([{"role": "user", "content": "hi"}]));
    req.body["tools"] = json!([{
        "type": "function",
        "function": {
            "name": "read_file",
            "description": "Read a file",
            "parameters": {
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"]
            }
        }
    }]);
    let result = mapper.map_request(&req).unwrap();
    let tools = result["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], "read_file");
    assert_eq!(tools[0]["description"], "Read a file");
    assert!(tools[0].get("input_schema").is_some());
}

#[test]
fn openai_to_claude_assistant_tool_calls() {
    let mapper = OpenAiToClaudeMapper;
    let req = openai_chat_request(json!([
        {"role": "user", "content": "weather?"},
        {
            "role": "assistant",
            "content": null,
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "arguments": "{\"city\":\"NYC\"}"
                }
            }]
        },
        {"role": "tool", "tool_call_id": "call_1", "content": "72°F"}
    ]));
    let result = mapper.map_request(&req).unwrap();
    let msgs = result["messages"].as_array().unwrap();
    // Assistant message has tool_use block
    assert_eq!(msgs[1]["content"][0]["type"], "tool_use");
    assert_eq!(msgs[1]["content"][0]["name"], "get_weather");
    // Tool result in user message
    assert_eq!(msgs[2]["role"], "user");
    assert_eq!(msgs[2]["content"][0]["type"], "tool_result");
}

#[test]
fn openai_to_claude_stop_string_to_array() {
    let mapper = OpenAiToClaudeMapper;
    let mut req = openai_chat_request(json!([{"role": "user", "content": "hi"}]));
    req.body["stop"] = json!("END");
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["stop_sequences"], json!(["END"]));
}

#[test]
fn openai_to_claude_stop_array_passthrough() {
    let mapper = OpenAiToClaudeMapper;
    let mut req = openai_chat_request(json!([{"role": "user", "content": "hi"}]));
    req.body["stop"] = json!(["A", "B"]);
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["stop_sequences"], json!(["A", "B"]));
}

#[test]
fn openai_to_claude_stream_flag() {
    let mapper = OpenAiToClaudeMapper;
    let mut req = openai_chat_request(json!([{"role": "user", "content": "hi"}]));
    req.body["stream"] = json!(true);
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["stream"], true);
}

#[test]
fn openai_to_claude_wrong_dialect_rejected() {
    let mapper = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::Claude,
        body: json!({"model": "x"}),
    };
    assert!(matches!(
        mapper.map_request(&req),
        Err(MappingError::UnmappableRequest { .. })
    ));
}

#[test]
fn openai_to_claude_source_target_dialects() {
    let mapper = OpenAiToClaudeMapper;
    assert_eq!(mapper.source_dialect(), Dialect::OpenAi);
    assert_eq!(mapper.target_dialect(), Dialect::Claude);
}

// =========================================================================
// 4. Claude → OpenAI mapping
// =========================================================================

#[test]
fn claude_to_openai_basic_user_message() {
    let mapper = ClaudeToOpenAiMapper;
    let req = claude_chat_request(json!([{"role": "user", "content": "Hello"}]), None);
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["messages"][0]["role"], "user");
    assert_eq!(result["messages"][0]["content"], "Hello");
}

#[test]
fn claude_to_openai_system_becomes_system_message() {
    let mapper = ClaudeToOpenAiMapper;
    let req = claude_chat_request(
        json!([{"role": "user", "content": "Hi"}]),
        Some("Be helpful."),
    );
    let result = mapper.map_request(&req).unwrap();
    let msgs = result["messages"].as_array().unwrap();
    assert_eq!(msgs[0]["role"], "system");
    assert_eq!(msgs[0]["content"], "Be helpful.");
    assert_eq!(msgs[1]["role"], "user");
}

#[test]
fn claude_to_openai_model_passthrough() {
    let mapper = ClaudeToOpenAiMapper;
    let req = claude_chat_request(json!([{"role": "user", "content": "hi"}]), None);
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["model"], "claude-3-5-sonnet-20241022");
}

#[test]
fn claude_to_openai_wrong_dialect_rejected() {
    let mapper = ClaudeToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({"model": "gpt-4"}),
    };
    assert!(matches!(
        mapper.map_request(&req),
        Err(MappingError::UnmappableRequest { .. })
    ));
}

#[test]
fn claude_to_openai_source_target_dialects() {
    let mapper = ClaudeToOpenAiMapper;
    assert_eq!(mapper.source_dialect(), Dialect::Claude);
    assert_eq!(mapper.target_dialect(), Dialect::OpenAi);
}

#[test]
fn claude_to_openai_response_tagged_as_openai() {
    let mapper = ClaudeToOpenAiMapper;
    let body = json!({"id": "msg_1", "choices": []});
    let resp = mapper.map_response(&body).unwrap();
    assert_eq!(resp.dialect, Dialect::OpenAi);
}

// =========================================================================
// 5. OpenAI → Gemini mapping
// =========================================================================

#[test]
fn openai_to_gemini_basic_user_message() {
    let mapper = OpenAiToGeminiMapper;
    let req = openai_chat_request(json!([{"role": "user", "content": "Hello"}]));
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["contents"][0]["role"], "user");
    assert_eq!(result["contents"][0]["parts"][0]["text"], "Hello");
}

#[test]
fn openai_to_gemini_system_to_system_instruction() {
    let mapper = OpenAiToGeminiMapper;
    let req = openai_chat_request(json!([
        {"role": "system", "content": "Be helpful."},
        {"role": "user", "content": "Hi"}
    ]));
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(
        result["system_instruction"]["parts"][0]["text"],
        "Be helpful."
    );
    assert_eq!(result["contents"].as_array().unwrap().len(), 1);
}

#[test]
fn openai_to_gemini_assistant_becomes_model() {
    let mapper = OpenAiToGeminiMapper;
    let req = openai_chat_request(json!([
        {"role": "user", "content": "Hi"},
        {"role": "assistant", "content": "Hello!"}
    ]));
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["contents"][1]["role"], "model");
    assert_eq!(result["contents"][1]["parts"][0]["text"], "Hello!");
}

#[test]
fn openai_to_gemini_generation_config() {
    let mapper = OpenAiToGeminiMapper;
    let mut req = openai_chat_request(json!([{"role": "user", "content": "hi"}]));
    req.body["max_tokens"] = json!(512);
    req.body["temperature"] = json!(0.3);
    req.body["top_p"] = json!(0.8);
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["generationConfig"]["maxOutputTokens"], 512);
    assert_eq!(result["generationConfig"]["temperature"], 0.3);
    assert_eq!(result["generationConfig"]["topP"], 0.8);
}

#[test]
fn openai_to_gemini_tools_to_function_declarations() {
    let mapper = OpenAiToGeminiMapper;
    let mut req = openai_chat_request(json!([{"role": "user", "content": "hi"}]));
    req.body["tools"] = json!([{
        "type": "function",
        "function": {
            "name": "search",
            "description": "Search the web",
            "parameters": {
                "type": "object",
                "properties": {"query": {"type": "string"}}
            }
        }
    }]);
    let result = mapper.map_request(&req).unwrap();
    let decl = &result["tools"][0]["function_declarations"][0];
    assert_eq!(decl["name"], "search");
    assert_eq!(decl["description"], "Search the web");
}

#[test]
fn openai_to_gemini_tool_calls_as_function_call() {
    let mapper = OpenAiToGeminiMapper;
    let req = openai_chat_request(json!([
        {"role": "user", "content": "Weather?"},
        {
            "role": "assistant",
            "content": null,
            "tool_calls": [{
                "id": "c1",
                "type": "function",
                "function": {"name": "get_weather", "arguments": "{\"city\":\"LA\"}"}
            }]
        }
    ]));
    let result = mapper.map_request(&req).unwrap();
    let model_msg = &result["contents"][1];
    assert_eq!(model_msg["role"], "model");
    assert_eq!(model_msg["parts"][0]["functionCall"]["name"], "get_weather");
}

#[test]
fn openai_to_gemini_wrong_dialect_rejected() {
    let mapper = OpenAiToGeminiMapper;
    let req = DialectRequest {
        dialect: Dialect::Gemini,
        body: json!({"model": "gemini"}),
    };
    assert!(mapper.map_request(&req).is_err());
}

#[test]
fn openai_to_gemini_source_target_dialects() {
    let mapper = OpenAiToGeminiMapper;
    assert_eq!(mapper.source_dialect(), Dialect::OpenAi);
    assert_eq!(mapper.target_dialect(), Dialect::Gemini);
}

// =========================================================================
// 6. Gemini → OpenAI mapping
// =========================================================================

#[test]
fn gemini_to_openai_basic_user_message() {
    let mapper = GeminiToOpenAiMapper;
    let req = gemini_chat_request(json!([
        {"role": "user", "parts": [{"text": "Hello"}]}
    ]));
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["messages"][0]["role"], "user");
    assert_eq!(result["messages"][0]["content"], "Hello");
}

#[test]
fn gemini_to_openai_system_instruction_becomes_system() {
    let mapper = GeminiToOpenAiMapper;
    let mut req = gemini_chat_request(json!([
        {"role": "user", "parts": [{"text": "Hi"}]}
    ]));
    req.body["system_instruction"] = json!({"parts": [{"text": "Be helpful."}]});
    let result = mapper.map_request(&req).unwrap();
    let msgs = result["messages"].as_array().unwrap();
    assert_eq!(msgs[0]["role"], "system");
    assert_eq!(msgs[0]["content"], "Be helpful.");
}

#[test]
fn gemini_to_openai_model_becomes_assistant() {
    let mapper = GeminiToOpenAiMapper;
    let req = gemini_chat_request(json!([
        {"role": "user", "parts": [{"text": "Hi"}]},
        {"role": "model", "parts": [{"text": "Hello!"}]}
    ]));
    let result = mapper.map_request(&req).unwrap();
    let msgs = result["messages"].as_array().unwrap();
    assert_eq!(msgs[1]["role"], "assistant");
    assert_eq!(msgs[1]["content"], "Hello!");
}

#[test]
fn gemini_to_openai_generation_config_flattened() {
    let mapper = GeminiToOpenAiMapper;
    let mut req = gemini_chat_request(json!([
        {"role": "user", "parts": [{"text": "hi"}]}
    ]));
    req.body["generationConfig"] = json!({
        "maxOutputTokens": 512,
        "temperature": 0.3,
        "topP": 0.8,
        "stopSequences": ["END"]
    });
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["max_tokens"], 512);
    assert_eq!(result["temperature"], 0.3);
    assert_eq!(result["top_p"], 0.8);
    assert_eq!(result["stop"], json!(["END"]));
}

#[test]
fn gemini_to_openai_function_declarations_to_tools() {
    let mapper = GeminiToOpenAiMapper;
    let mut req = gemini_chat_request(json!([
        {"role": "user", "parts": [{"text": "hi"}]}
    ]));
    req.body["tools"] = json!([{
        "function_declarations": [{
            "name": "calc",
            "description": "Calculate",
            "parameters": {"type": "object", "properties": {"expr": {"type": "string"}}}
        }]
    }]);
    let result = mapper.map_request(&req).unwrap();
    let tools = result["tools"].as_array().unwrap();
    assert_eq!(tools[0]["type"], "function");
    assert_eq!(tools[0]["function"]["name"], "calc");
}

#[test]
fn gemini_to_openai_function_call_to_tool_calls() {
    let mapper = GeminiToOpenAiMapper;
    let req = gemini_chat_request(json!([
        {"role": "user", "parts": [{"text": "Calculate"}]},
        {"role": "model", "parts": [{"functionCall": {"name": "calc", "args": {"expr": "2+2"}}}]}
    ]));
    let result = mapper.map_request(&req).unwrap();
    let msgs = result["messages"].as_array().unwrap();
    assert_eq!(msgs[1]["role"], "assistant");
    assert!(msgs[1]["tool_calls"].as_array().is_some());
    assert_eq!(msgs[1]["tool_calls"][0]["function"]["name"], "calc");
}

#[test]
fn gemini_to_openai_function_response_to_tool_message() {
    let mapper = GeminiToOpenAiMapper;
    let req = gemini_chat_request(json!([
        {"role": "user", "parts": [{"functionResponse": {"name": "calc", "response": "4"}}]}
    ]));
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["messages"][0]["role"], "tool");
    assert_eq!(result["messages"][0]["name"], "calc");
}

#[test]
fn gemini_to_openai_inline_data_to_image_url() {
    let mapper = GeminiToOpenAiMapper;
    let req = gemini_chat_request(json!([{
        "role": "user",
        "parts": [
            {"text": "What is this?"},
            {"inlineData": {"mimeType": "image/png", "data": "base64data"}}
        ]
    }]));
    let result = mapper.map_request(&req).unwrap();
    let content = result["messages"][0]["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[1]["type"], "image_url");
    assert!(
        content[1]["image_url"]["url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,")
    );
}

#[test]
fn gemini_to_openai_wrong_dialect_rejected() {
    let mapper = GeminiToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({"model": "gpt-4"}),
    };
    assert!(mapper.map_request(&req).is_err());
}

#[test]
fn gemini_to_openai_source_target_dialects() {
    let mapper = GeminiToOpenAiMapper;
    assert_eq!(mapper.source_dialect(), Dialect::Gemini);
    assert_eq!(mapper.target_dialect(), Dialect::OpenAi);
}

// =========================================================================
// 7. Lossy detection: detect when mapping loses information
// =========================================================================

#[test]
fn lossy_thinking_block_dropped_claude_to_openai_ir() {
    let ir = thinking_ir();
    let mapper = default_ir_mapper(Dialect::Claude, Dialect::OpenAi).unwrap();
    let mapped = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &ir)
        .unwrap();
    // Thinking blocks should be dropped
    let assistant = mapped.last_assistant().unwrap();
    let has_thinking = assistant
        .content
        .iter()
        .any(|b| matches!(b, IrContentBlock::Thinking { .. }));
    assert!(!has_thinking, "thinking block should be dropped for OpenAI");
    // But the text answer should survive
    assert!(assistant.text_content().contains("4"));
}

#[test]
fn lossy_thinking_block_dropped_openai_to_gemini_ir() {
    let ir = thinking_ir();
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Gemini).unwrap();
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &ir)
        .unwrap();
    let assistant = mapped.last_assistant().unwrap();
    let has_thinking = assistant
        .content
        .iter()
        .any(|b| matches!(b, IrContentBlock::Thinking { .. }));
    assert!(!has_thinking, "thinking block should be dropped for Gemini");
}

#[test]
fn lossy_validation_detects_field_loss() {
    let v = DefaultMappingValidator::new();
    let original = json!({"model": "gpt-4", "temperature": 0.7, "logprobs": true});
    let roundtripped = json!({"model": "gpt-4", "temperature": 0.7});
    let result = v.validate_roundtrip(&original, &roundtripped);
    assert!(!result.is_lossless());
    assert!(result.lost_fields.contains(&"logprobs".into()));
}

#[test]
fn lossy_validation_detects_value_change() {
    let v = DefaultMappingValidator::new();
    let original = json!({"model": "gpt-4", "temperature": 0.7});
    let roundtripped = json!({"model": "gpt-4", "temperature": 0.5});
    let result = v.validate_roundtrip(&original, &roundtripped);
    assert!(!result.is_lossless());
    assert!(result.changed_fields.contains(&"temperature".into()));
}

#[test]
fn lossy_map_error_variant() {
    let err = MapError::LossyConversion {
        field: "thinking".into(),
        reason: "target has no thinking block".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("thinking"));
    assert!(msg.contains("lossy"));
}

// =========================================================================
// 8. Unsupported features: early error for unmappable features
// =========================================================================

#[test]
fn unsupported_pair_on_openai_claude_mapper() {
    let mapper = OpenAiClaudeIrMapper;
    let ir = simple_ir();
    let result = mapper.map_request(Dialect::Gemini, Dialect::Kimi, &ir);
    assert!(matches!(result, Err(MapError::UnsupportedPair { .. })));
}

#[test]
fn unsupported_pair_on_openai_gemini_mapper() {
    let mapper = OpenAiGeminiIrMapper;
    let ir = simple_ir();
    let result = mapper.map_request(Dialect::Claude, Dialect::Kimi, &ir);
    assert!(matches!(result, Err(MapError::UnsupportedPair { .. })));
}

#[test]
fn unsupported_pair_on_response_mapping() {
    let mapper = OpenAiClaudeIrMapper;
    let ir = simple_ir();
    let result = mapper.map_response(Dialect::Gemini, Dialect::Codex, &ir);
    assert!(matches!(result, Err(MapError::UnsupportedPair { .. })));
}

#[test]
fn json_mapper_rejects_wrong_source_dialect() {
    let mapper = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::Gemini,
        body: json!({"model": "x", "messages": []}),
    };
    assert!(mapper.map_request(&req).is_err());
}

#[test]
fn json_mapper_rejects_non_object_body() {
    let mapper = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!("string body"),
    };
    assert!(matches!(
        mapper.map_request(&req),
        Err(MappingError::UnmappableRequest { .. })
    ));
}

#[test]
fn json_mapper_unknown_role_rejected() {
    let mapper = OpenAiToClaudeMapper;
    let req = openai_chat_request(json!([{"role": "developer", "content": "hi"}]));
    assert!(matches!(
        mapper.map_request(&req),
        Err(MappingError::IncompatibleTypes { .. })
    ));
}

#[test]
fn gemini_mapper_unknown_role_rejected() {
    let mapper = GeminiToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::Gemini,
        body: json!({
            "model": "gemini",
            "contents": [{"role": "unknown", "parts": [{"text": "hi"}]}]
        }),
    };
    assert!(matches!(
        mapper.map_request(&req),
        Err(MappingError::IncompatibleTypes { .. })
    ));
}

#[test]
fn unsupported_capability_error_variant() {
    let err = MappingError::UnsupportedCapability {
        capability: "logprobs".into(),
        source_dialect: Dialect::Claude,
        target_dialect: Dialect::OpenAi,
    };
    assert!(err.to_string().contains("logprobs"));
}

// =========================================================================
// 9. System messages: map system prompts across dialects
// =========================================================================

#[test]
fn system_openai_to_claude_extracted_to_top_level() {
    let mapper = OpenAiToClaudeMapper;
    let req = openai_chat_request(json!([
        {"role": "system", "content": "You are an expert."},
        {"role": "user", "content": "Help me"}
    ]));
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["system"], "You are an expert.");
}

#[test]
fn system_claude_to_openai_becomes_first_message() {
    let mapper = ClaudeToOpenAiMapper;
    let req = claude_chat_request(
        json!([{"role": "user", "content": "hi"}]),
        Some("Expert mode."),
    );
    let result = mapper.map_request(&req).unwrap();
    let msgs = result["messages"].as_array().unwrap();
    assert_eq!(msgs[0]["role"], "system");
    assert_eq!(msgs[0]["content"], "Expert mode.");
}

#[test]
fn system_openai_to_gemini_becomes_system_instruction() {
    let mapper = OpenAiToGeminiMapper;
    let req = openai_chat_request(json!([
        {"role": "system", "content": "Be precise."},
        {"role": "user", "content": "hi"}
    ]));
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(
        result["system_instruction"]["parts"][0]["text"],
        "Be precise."
    );
}

#[test]
fn system_gemini_to_openai_becomes_system_message() {
    let mapper = GeminiToOpenAiMapper;
    let mut req = gemini_chat_request(json!([
        {"role": "user", "parts": [{"text": "hi"}]}
    ]));
    req.body["system_instruction"] = json!({"parts": [{"text": "Be precise."}]});
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["messages"][0]["role"], "system");
    assert_eq!(result["messages"][0]["content"], "Be precise.");
}

#[test]
fn system_ir_preserved_openai_to_claude() {
    let ir = simple_ir();
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    let sys = mapped.system_message();
    assert!(sys.is_some());
    assert!(sys.unwrap().text_content().contains("helpful"));
}

#[test]
fn system_ir_preserved_openai_to_gemini() {
    let ir = simple_ir();
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Gemini).unwrap();
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &ir)
        .unwrap();
    let sys = mapped.system_message();
    assert!(sys.is_some());
    assert!(sys.unwrap().text_content().contains("helpful"));
}

// =========================================================================
// 10. Tool schemas: map function definitions
// =========================================================================

#[test]
fn tool_schema_openai_function_to_claude_tool() {
    let mapper = OpenAiToClaudeMapper;
    let mut req = openai_chat_request(json!([{"role": "user", "content": "hi"}]));
    req.body["tools"] = json!([{
        "type": "function",
        "function": {
            "name": "get_weather",
            "description": "Get weather for a city",
            "parameters": {
                "type": "object",
                "properties": {
                    "city": {"type": "string", "description": "City name"},
                    "unit": {"type": "string", "enum": ["celsius", "fahrenheit"]}
                },
                "required": ["city"]
            }
        }
    }]);
    let result = mapper.map_request(&req).unwrap();
    let tool = &result["tools"][0];
    assert_eq!(tool["name"], "get_weather");
    assert_eq!(tool["description"], "Get weather for a city");
    assert!(tool["input_schema"]["properties"]["city"].is_object());
    assert!(tool["input_schema"]["properties"]["unit"].is_object());
}

#[test]
fn tool_schema_openai_function_to_gemini_declaration() {
    let mapper = OpenAiToGeminiMapper;
    let mut req = openai_chat_request(json!([{"role": "user", "content": "hi"}]));
    req.body["tools"] = json!([{
        "type": "function",
        "function": {
            "name": "search",
            "description": "Search",
            "parameters": {"type": "object", "properties": {"q": {"type": "string"}}}
        }
    }]);
    let result = mapper.map_request(&req).unwrap();
    let decl = &result["tools"][0]["function_declarations"][0];
    assert_eq!(decl["name"], "search");
    assert!(decl["parameters"]["properties"]["q"].is_object());
}

#[test]
fn tool_schema_gemini_to_openai_function() {
    let mapper = GeminiToOpenAiMapper;
    let mut req = gemini_chat_request(json!([
        {"role": "user", "parts": [{"text": "hi"}]}
    ]));
    req.body["tools"] = json!([{
        "function_declarations": [{
            "name": "translate",
            "description": "Translate text",
            "parameters": {
                "type": "object",
                "properties": {
                    "text": {"type": "string"},
                    "target_lang": {"type": "string"}
                }
            }
        }]
    }]);
    let result = mapper.map_request(&req).unwrap();
    let tool = &result["tools"][0];
    assert_eq!(tool["type"], "function");
    assert_eq!(tool["function"]["name"], "translate");
    assert!(tool["function"]["parameters"]["properties"]["text"].is_object());
}

#[test]
fn tool_schema_multiple_tools() {
    let mapper = OpenAiToClaudeMapper;
    let mut req = openai_chat_request(json!([{"role": "user", "content": "hi"}]));
    req.body["tools"] = json!([
        {"type": "function", "function": {"name": "tool_a", "description": "A", "parameters": {"type": "object", "properties": {}}}},
        {"type": "function", "function": {"name": "tool_b", "description": "B", "parameters": {"type": "object", "properties": {}}}}
    ]);
    let result = mapper.map_request(&req).unwrap();
    let tools = result["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0]["name"], "tool_a");
    assert_eq!(tools[1]["name"], "tool_b");
}

#[test]
fn tool_schema_missing_function_field_error() {
    let mapper = OpenAiToClaudeMapper;
    let mut req = openai_chat_request(json!([{"role": "user", "content": "hi"}]));
    req.body["tools"] = json!([{"type": "function"}]); // missing "function" key
    assert!(matches!(
        mapper.map_request(&req),
        Err(MappingError::IncompatibleTypes { .. })
    ));
}

// =========================================================================
// 11. Streaming events: map delta events across dialects
// =========================================================================

#[test]
fn event_openai_to_claude_assistant_delta() {
    let mapper = OpenAiToClaudeMapper;
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta {
            text: "token".into(),
        },
        ext: None,
    };
    let result = mapper.map_event(&event).unwrap();
    assert_eq!(result["type"], "content_block_delta");
    assert_eq!(result["delta"]["text"], "token");
}

#[test]
fn event_openai_to_claude_assistant_message() {
    let mapper = OpenAiToClaudeMapper;
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "Done.".into(),
        },
        ext: None,
    };
    let result = mapper.map_event(&event).unwrap();
    assert_eq!(result["type"], "message");
    assert_eq!(result["content"][0]["text"], "Done.");
}

#[test]
fn event_openai_to_claude_tool_call() {
    let mapper = OpenAiToClaudeMapper;
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: Some("tu_1".into()),
            parent_tool_use_id: None,
            input: json!({"command": "ls"}),
        },
        ext: None,
    };
    let result = mapper.map_event(&event).unwrap();
    assert_eq!(result["type"], "content_block_start");
    assert_eq!(result["content_block"]["name"], "bash");
    assert_eq!(result["content_block"]["id"], "tu_1");
}

#[test]
fn event_openai_to_claude_tool_result() {
    let mapper = OpenAiToClaudeMapper;
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: Some("tu_1".into()),
            output: json!("output text"),
            is_error: false,
        },
        ext: None,
    };
    let result = mapper.map_event(&event).unwrap();
    assert_eq!(result["type"], "tool_result");
    assert_eq!(result["is_error"], false);
}

#[test]
fn event_openai_to_gemini_assistant_delta() {
    let mapper = OpenAiToGeminiMapper;
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta {
            text: "chunk".into(),
        },
        ext: None,
    };
    let result = mapper.map_event(&event).unwrap();
    assert_eq!(result["candidates"][0]["content"]["role"], "model");
    assert_eq!(
        result["candidates"][0]["content"]["parts"][0]["text"],
        "chunk"
    );
}

#[test]
fn event_gemini_to_openai_assistant_delta() {
    let mapper = GeminiToOpenAiMapper;
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: "tok".into() },
        ext: None,
    };
    let result = mapper.map_event(&event).unwrap();
    assert_eq!(result["object"], "chat.completion.chunk");
    assert_eq!(result["choices"][0]["delta"]["content"], "tok");
}

#[test]
fn event_gemini_to_openai_tool_call() {
    let mapper = GeminiToOpenAiMapper;
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "search".into(),
            tool_use_id: Some("c1".into()),
            parent_tool_use_id: None,
            input: json!({"q": "rust"}),
        },
        ext: None,
    };
    let result = mapper.map_event(&event).unwrap();
    assert_eq!(result["object"], "chat.completion.chunk");
    let tc = &result["choices"][0]["delta"]["tool_calls"][0];
    assert_eq!(tc["function"]["name"], "search");
    assert_eq!(tc["id"], "c1");
}

#[test]
fn event_identity_mapper_serializes_event() {
    let mapper = IdentityMapper;
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Warning {
            message: "slow".into(),
        },
        ext: None,
    };
    let result = mapper.map_event(&event).unwrap();
    assert_eq!(result["type"], "warning");
    assert_eq!(result["message"], "slow");
}

#[test]
fn event_openai_to_claude_warning_fallback() {
    let mapper = OpenAiToClaudeMapper;
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Warning {
            message: "rate limit".into(),
        },
        ext: None,
    };
    let result = mapper.map_event(&event).unwrap();
    assert_eq!(result["type"], "warning");
}

// =========================================================================
// 12. Round trip: A→B→A preserves semantics where possible
// =========================================================================

#[test]
fn roundtrip_openai_claude_openai_simple_ir() {
    let ir = simple_ir();
    let fwd = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
    let bwd = default_ir_mapper(Dialect::Claude, Dialect::OpenAi).unwrap();
    let intermediate = fwd
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    let roundtripped = bwd
        .map_request(Dialect::Claude, Dialect::OpenAi, &intermediate)
        .unwrap();
    // User and assistant messages should survive
    assert!(!roundtripped.messages_by_role(IrRole::User).is_empty());
    assert!(roundtripped.last_assistant().is_some());
    assert!(
        roundtripped
            .last_assistant()
            .unwrap()
            .text_content()
            .contains("Hi there")
    );
}

#[test]
fn roundtrip_openai_gemini_openai_simple_ir() {
    let ir = simple_ir();
    let fwd = default_ir_mapper(Dialect::OpenAi, Dialect::Gemini).unwrap();
    let bwd = default_ir_mapper(Dialect::Gemini, Dialect::OpenAi).unwrap();
    let intermediate = fwd
        .map_request(Dialect::OpenAi, Dialect::Gemini, &ir)
        .unwrap();
    let roundtripped = bwd
        .map_request(Dialect::Gemini, Dialect::OpenAi, &intermediate)
        .unwrap();
    assert!(!roundtripped.messages_by_role(IrRole::User).is_empty());
    assert!(roundtripped.last_assistant().is_some());
}

#[test]
fn roundtrip_openai_claude_openai_tool_calls() {
    let ir = tool_call_ir();
    let fwd = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
    let bwd = default_ir_mapper(Dialect::Claude, Dialect::OpenAi).unwrap();
    let intermediate = fwd
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    let roundtripped = bwd
        .map_request(Dialect::Claude, Dialect::OpenAi, &intermediate)
        .unwrap();
    assert_eq!(
        ir.tool_calls().len(),
        roundtripped.tool_calls().len(),
        "tool call count mismatch after round-trip"
    );
}

#[test]
fn roundtrip_openai_gemini_openai_json() {
    let o2g = OpenAiToGeminiMapper;
    let g2o = GeminiToOpenAiMapper;
    let openai_req = json!({
        "model": "gpt-4",
        "messages": [
            {"role": "system", "content": "Be helpful"},
            {"role": "user", "content": "Hello"}
        ]
    });
    let gemini_req = o2g
        .map_request(&DialectRequest {
            dialect: Dialect::OpenAi,
            body: openai_req,
        })
        .unwrap();
    let back = g2o
        .map_request(&DialectRequest {
            dialect: Dialect::Gemini,
            body: gemini_req,
        })
        .unwrap();
    let msgs = back["messages"].as_array().unwrap();
    assert_eq!(msgs[0]["role"], "system");
    assert_eq!(msgs[0]["content"], "Be helpful");
    assert_eq!(msgs[1]["role"], "user");
    assert_eq!(msgs[1]["content"], "Hello");
}

#[test]
fn roundtrip_tool_definitions_openai_gemini() {
    let o2g = OpenAiToGeminiMapper;
    let g2o = GeminiToOpenAiMapper;
    let openai_req = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [{
            "type": "function",
            "function": {
                "name": "search",
                "description": "Search",
                "parameters": {"type": "object", "properties": {"q": {"type": "string"}}}
            }
        }]
    });
    let gemini_req = o2g
        .map_request(&DialectRequest {
            dialect: Dialect::OpenAi,
            body: openai_req,
        })
        .unwrap();
    let back = g2o
        .map_request(&DialectRequest {
            dialect: Dialect::Gemini,
            body: gemini_req,
        })
        .unwrap();
    assert_eq!(back["tools"][0]["function"]["name"], "search");
}

#[test]
fn roundtrip_identity_is_lossless() {
    let ir = simple_ir();
    let mapper = IrIdentityMapper;
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::OpenAi, &ir)
        .unwrap();
    assert_eq!(ir, mapped);
}

#[test]
fn roundtrip_validation_identical_is_lossless() {
    let v = DefaultMappingValidator::new();
    let val = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let result = v.validate_roundtrip(&val, &val);
    assert!(result.is_lossless());
}

// =========================================================================
// 13. Error taxonomy: mapping errors produce correct error codes
// =========================================================================

#[test]
fn error_unsupported_pair_has_correct_variant() {
    let err = MapError::UnsupportedPair {
        from: Dialect::Kimi,
        to: Dialect::Copilot,
    };
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
    assert!(err.to_string().contains("Kimi"));
}

#[test]
fn error_lossy_conversion_has_correct_variant() {
    let err = MapError::LossyConversion {
        field: "system_instruction".into(),
        reason: "flattened".into(),
    };
    assert!(matches!(err, MapError::LossyConversion { .. }));
    assert!(err.to_string().contains("lossy"));
}

#[test]
fn error_unmappable_tool_has_correct_variant() {
    let err = MapError::UnmappableTool {
        name: "computer_use".into(),
        reason: "not supported".into(),
    };
    assert!(matches!(err, MapError::UnmappableTool { .. }));
    assert!(err.to_string().contains("computer_use"));
}

#[test]
fn error_incompatible_capability_has_correct_variant() {
    let err = MapError::IncompatibleCapability {
        capability: "vision".into(),
        reason: "no image support".into(),
    };
    assert!(matches!(err, MapError::IncompatibleCapability { .. }));
    assert!(err.to_string().contains("vision"));
}

#[test]
fn error_unmappable_content_has_correct_variant() {
    let err = MapError::UnmappableContent {
        field: "system".into(),
        reason: "image blocks in system prompt".into(),
    };
    assert!(matches!(err, MapError::UnmappableContent { .. }));
    assert!(err.to_string().contains("image blocks"));
}

#[test]
fn error_mapping_error_unsupported_capability() {
    let err = MappingError::UnsupportedCapability {
        capability: "logprobs".into(),
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
    };
    assert!(err.to_string().contains("logprobs"));
    assert!(err.to_string().contains("OpenAI"));
    assert!(err.to_string().contains("Claude"));
}

#[test]
fn error_mapping_error_incompatible_types() {
    let err = MappingError::IncompatibleTypes {
        source_type: "function_call".into(),
        target_type: "tool_use".into(),
        reason: "schema mismatch".into(),
    };
    assert!(err.to_string().contains("function_call"));
    assert!(err.to_string().contains("tool_use"));
}

#[test]
fn error_mapping_error_fidelity_loss() {
    let err = MappingError::FidelityLoss {
        field: "thinking".into(),
        source_dialect: Dialect::Claude,
        target_dialect: Dialect::OpenAi,
        detail: "no native thinking block".into(),
    };
    assert!(err.to_string().contains("fidelity loss"));
    assert!(err.to_string().contains("thinking"));
}

#[test]
fn error_mapping_error_unmappable_request() {
    let err = MappingError::UnmappableRequest {
        reason: "empty body".into(),
    };
    assert!(err.to_string().contains("empty body"));
}

#[test]
fn error_map_error_serialize_roundtrip() {
    let err = MapError::UnsupportedPair {
        from: Dialect::OpenAi,
        to: Dialect::Claude,
    };
    let json_str = serde_json::to_string(&err).unwrap();
    let back: MapError = serde_json::from_str(&json_str).unwrap();
    assert_eq!(err, back);
}

#[test]
fn error_map_error_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MapError>();
}

#[test]
fn error_mapping_error_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MappingError>();
}

#[test]
fn error_validation_pipeline_mapping_failure() {
    let pipe = ValidationPipeline::new(
        DefaultMappingValidator::new(),
        Dialect::OpenAi,
        Dialect::Claude,
    );
    let req = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let result = pipe.run(&req, |_| Err("forced error".into()));
    let post = result.post.unwrap();
    assert!(!post.is_valid());
    assert_eq!(post.issues[0].code, "mapping_failed");
}

// =========================================================================
// 14. Edge cases: empty messages, very long content, unicode
// =========================================================================

#[test]
fn edge_empty_messages_array() {
    let mapper = OpenAiToClaudeMapper;
    let req = openai_chat_request(json!([]));
    // Should succeed, producing empty messages
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["messages"].as_array().unwrap().len(), 0);
}

#[test]
fn edge_empty_conversation_ir() {
    let ir = IrConversation::new();
    for (from, to) in supported_ir_pairs() {
        let mapper = default_ir_mapper(from, to).unwrap();
        let mapped = mapper.map_request(from, to, &ir).unwrap();
        assert!(mapped.is_empty(), "empty conv non-empty for {from}→{to}");
    }
}

#[test]
fn edge_very_long_content() {
    let long_text = "A".repeat(100_000);
    let mapper = OpenAiToClaudeMapper;
    let req = openai_chat_request(json!([{"role": "user", "content": long_text}]));
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(
        result["messages"][0]["content"].as_str().unwrap().len(),
        100_000
    );
}

#[test]
fn edge_unicode_content_preserved() {
    let unicode_text = "こんにちは世界 🌍 Ñoño café résumé naïve 你好 مرحبا";
    let mapper = OpenAiToClaudeMapper;
    let req = openai_chat_request(json!([{"role": "user", "content": unicode_text}]));
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["messages"][0]["content"], unicode_text);
}

#[test]
fn edge_unicode_in_gemini_roundtrip() {
    let text = "日本語テスト 🎉";
    let o2g = OpenAiToGeminiMapper;
    let g2o = GeminiToOpenAiMapper;
    let openai_req = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": text}]
    });
    let gemini_req = o2g
        .map_request(&DialectRequest {
            dialect: Dialect::OpenAi,
            body: openai_req,
        })
        .unwrap();
    let back = g2o
        .map_request(&DialectRequest {
            dialect: Dialect::Gemini,
            body: gemini_req,
        })
        .unwrap();
    assert_eq!(back["messages"][0]["content"], text);
}

#[test]
fn edge_empty_string_content() {
    let mapper = OpenAiToClaudeMapper;
    let req = openai_chat_request(json!([{"role": "user", "content": ""}]));
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["messages"][0]["content"], "");
}

#[test]
fn edge_null_content_handled() {
    let mapper = OpenAiToClaudeMapper;
    let req = openai_chat_request(json!([{"role": "user", "content": null}]));
    // Should not panic
    let result = mapper.map_request(&req);
    assert!(result.is_ok());
}

#[test]
fn edge_nested_json_in_tool_input() {
    let mapper = OpenAiToClaudeMapper;
    let req = openai_chat_request(json!([
        {"role": "user", "content": "process this"},
        {
            "role": "assistant",
            "content": null,
            "tool_calls": [{
                "id": "call_deep",
                "type": "function",
                "function": {
                    "name": "process",
                    "arguments": "{\"data\":{\"nested\":{\"deep\":true}},\"list\":[1,2,3]}"
                }
            }]
        }
    ]));
    let result = mapper.map_request(&req).unwrap();
    let tool_block = &result["messages"][1]["content"][0];
    assert_eq!(tool_block["input"]["data"]["nested"]["deep"], true);
    assert_eq!(tool_block["input"]["list"], json!([1, 2, 3]));
}

#[test]
fn edge_special_characters_in_tool_name() {
    let ir = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "do it"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "c1".into(),
                name: "my-tool_v2.0".into(),
                input: json!({}),
            }],
        ),
    ]);
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    let tools = mapped.tool_calls();
    assert_eq!(tools.len(), 1);
    if let IrContentBlock::ToolUse { name, .. } = tools[0] {
        assert_eq!(name, "my-tool_v2.0");
    }
}

#[test]
fn edge_single_message_conversation() {
    let ir = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "just one")]);
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    assert_eq!(mapped.len(), 1);
    assert_eq!(mapped.messages[0].text_content(), "just one");
}

#[test]
fn edge_many_messages_preserves_count() {
    let mut msgs = Vec::new();
    for i in 0..50 {
        let role = if i % 2 == 0 {
            IrRole::User
        } else {
            IrRole::Assistant
        };
        msgs.push(IrMessage::text(role, format!("msg {i}")));
    }
    let ir = IrConversation::from_messages(msgs);
    let mapper = IrIdentityMapper;
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::OpenAi, &ir)
        .unwrap();
    assert_eq!(mapped.len(), 50);
}

#[test]
fn edge_dialect_request_clone() {
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({"model": "gpt-4"}),
    };
    let cloned = req.clone();
    assert_eq!(cloned.body, req.body);
}

#[test]
fn edge_dialect_response_clone() {
    let resp = DialectResponse {
        dialect: Dialect::Claude,
        body: json!({"content": []}),
    };
    let cloned = resp.clone();
    assert_eq!(cloned.body, resp.body);
    assert_eq!(cloned.dialect, Dialect::Claude);
}

#[test]
fn edge_validation_severity_display() {
    assert_eq!(ValidationSeverity::Info.to_string(), "info");
    assert_eq!(ValidationSeverity::Warning.to_string(), "warning");
    assert_eq!(ValidationSeverity::Error.to_string(), "error");
}

#[test]
fn edge_validation_non_object_input() {
    let v = DefaultMappingValidator::new();
    let result = v.validate_pre_mapping(Dialect::OpenAi, &json!(42));
    assert!(!result.is_valid());
    assert_eq!(result.field_coverage, 0.0);
}

#[test]
fn edge_validation_empty_messages_warning() {
    let v = DefaultMappingValidator::new();
    let req = json!({"model": "gpt-4", "messages": []});
    let result = v.validate_pre_mapping(Dialect::OpenAi, &req);
    // Valid (required fields present) but should have warning
    assert!(result.is_valid());
    assert!(
        result
            .issues
            .iter()
            .any(|i| i.code == "empty_messages" && i.severity == ValidationSeverity::Warning)
    );
}

#[test]
fn edge_multiline_system_prompt() {
    let mapper = OpenAiToClaudeMapper;
    let req = openai_chat_request(json!([
        {"role": "system", "content": "Line 1\nLine 2\nLine 3"},
        {"role": "user", "content": "Go"}
    ]));
    let result = mapper.map_request(&req).unwrap();
    assert!(
        result["system"]
            .as_str()
            .unwrap()
            .contains("Line 1\nLine 2\nLine 3")
    );
}

#[test]
fn edge_tool_result_with_error_flag() {
    let mapper = OpenAiToClaudeMapper;
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: Some("tu_err".into()),
            output: json!("command not found"),
            is_error: true,
        },
        ext: None,
    };
    let result = mapper.map_event(&event).unwrap();
    assert_eq!(result["is_error"], true);
}

#[test]
fn edge_image_content_in_openai_to_gemini() {
    let mapper = OpenAiToGeminiMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "What is this?"},
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,iVBOR"}}
                ]
            }]
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    let parts = result["contents"][0]["parts"].as_array().unwrap();
    assert_eq!(parts[0]["text"], "What is this?");
    assert!(parts[1].get("inlineData").is_some());
    assert_eq!(parts[1]["inlineData"]["mimeType"], "image/png");
}

#[test]
fn edge_top_p_mapped_openai_to_claude() {
    let mapper = OpenAiToClaudeMapper;
    let mut req = openai_chat_request(json!([{"role": "user", "content": "hi"}]));
    req.body["top_p"] = json!(0.95);
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["top_p"], 0.95);
}
