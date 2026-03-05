#![allow(clippy::all)]
#![allow(unknown_lints)]

//! Comprehensive test suite for the abp-mapper crate.
//!
//! 130+ tests covering identity mapping, factory selection, cross-dialect
//! JSON mappers, IR mappers, error types, validation, edge cases,
//! error propagation, and deterministic behaviour.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_core::{AgentEvent, AgentEventKind};
use abp_dialect::Dialect;
use abp_mapper::validation::{
    DefaultMappingValidator, MappingValidator, ValidationPipeline, ValidationSeverity,
};
use abp_mapper::{
    ClaudeGeminiIrMapper, ClaudeKimiIrMapper, ClaudeToOpenAiMapper, CodexClaudeIrMapper,
    DialectRequest, DialectResponse, GeminiKimiIrMapper, GeminiToOpenAiMapper, IdentityMapper,
    IrIdentityMapper, IrMapper, MapError, Mapper, MappingError, OpenAiClaudeIrMapper,
    OpenAiCodexIrMapper, OpenAiCopilotIrMapper, OpenAiGeminiIrMapper, OpenAiKimiIrMapper,
    OpenAiToClaudeMapper, OpenAiToGeminiMapper, default_ir_mapper, supported_ir_pairs,
};
use chrono::Utc;
use serde_json::{Value, json};

// ═══════════════════════════════════════════════════════════════════════════
// §1 IdentityMapper: passthrough fidelity, all message types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn identity_request_passthrough_simple() {
    let m = IdentityMapper;
    let body = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: body.clone(),
    };
    assert_eq!(m.map_request(&req).unwrap(), body);
}

#[test]
fn identity_request_passthrough_empty_object() {
    let m = IdentityMapper;
    let req = DialectRequest {
        dialect: Dialect::Claude,
        body: json!({}),
    };
    assert_eq!(m.map_request(&req).unwrap(), json!({}));
}

#[test]
fn identity_request_passthrough_any_dialect() {
    let m = IdentityMapper;
    for &d in Dialect::all() {
        let body = json!({"d": d.label()});
        let req = DialectRequest {
            dialect: d,
            body: body.clone(),
        };
        assert_eq!(m.map_request(&req).unwrap(), body);
    }
}

#[test]
fn identity_response_passthrough() {
    let m = IdentityMapper;
    let body = json!({"id": "x", "choices": [{"message": {"content": "hi"}}]});
    let resp = m.map_response(&body).unwrap();
    assert_eq!(resp.body, body);
    assert_eq!(resp.dialect, Dialect::OpenAi);
}

#[test]
fn identity_response_preserves_nested() {
    let m = IdentityMapper;
    let body = json!({"a": {"b": {"c": [1, 2, 3]}}});
    let resp = m.map_response(&body).unwrap();
    assert_eq!(resp.body["a"]["b"]["c"][2], 3);
}

#[test]
fn identity_event_assistant_message() {
    let m = IdentityMapper;
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext: None,
    };
    let v = m.map_event(&ev).unwrap();
    assert_eq!(v["text"], "hello");
}

#[test]
fn identity_event_tool_call() {
    let m = IdentityMapper;
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "read".into(),
            tool_use_id: Some("t1".into()),
            parent_tool_use_id: None,
            input: json!({"path": "a.rs"}),
        },
        ext: None,
    };
    let v = m.map_event(&ev).unwrap();
    assert_eq!(v["tool_name"], "read");
}

#[test]
fn identity_event_tool_result() {
    let m = IdentityMapper;
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: Some("tr1".into()),
            output: json!("ok"),
            is_error: false,
        },
        ext: None,
    };
    let v = m.map_event(&ev).unwrap();
    assert_eq!(v["output"], "ok");
    assert_eq!(v["is_error"], false);
}

#[test]
fn identity_event_warning() {
    let m = IdentityMapper;
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Warning {
            message: "slow".into(),
        },
        ext: None,
    };
    let v = m.map_event(&ev).unwrap();
    assert_eq!(v["type"], "warning");
}

#[test]
fn identity_event_run_started() {
    let m = IdentityMapper;
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    };
    let v = m.map_event(&ev).unwrap();
    assert_eq!(v["message"], "go");
}

#[test]
fn identity_source_target_dialects() {
    let m = IdentityMapper;
    assert_eq!(m.source_dialect(), Dialect::OpenAi);
    assert_eq!(m.target_dialect(), Dialect::OpenAi);
}

#[test]
fn identity_request_null_body() {
    let m = IdentityMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: Value::Null,
    };
    assert_eq!(m.map_request(&req).unwrap(), Value::Null);
}

#[test]
fn identity_request_array_body() {
    let m = IdentityMapper;
    let body = json!([1, 2, 3]);
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: body.clone(),
    };
    assert_eq!(m.map_request(&req).unwrap(), body);
}

// ═══════════════════════════════════════════════════════════════════════════
// §2 MapperFactory: correct mapper selection for each dialect pair
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn factory_identity_for_same_dialect() {
    for &d in Dialect::all() {
        let mapper = default_ir_mapper(d, d);
        assert!(mapper.is_some(), "no identity mapper for {d}");
    }
}

#[test]
fn factory_openai_claude() {
    assert!(default_ir_mapper(Dialect::OpenAi, Dialect::Claude).is_some());
    assert!(default_ir_mapper(Dialect::Claude, Dialect::OpenAi).is_some());
}

#[test]
fn factory_openai_gemini() {
    assert!(default_ir_mapper(Dialect::OpenAi, Dialect::Gemini).is_some());
    assert!(default_ir_mapper(Dialect::Gemini, Dialect::OpenAi).is_some());
}

#[test]
fn factory_claude_gemini() {
    assert!(default_ir_mapper(Dialect::Claude, Dialect::Gemini).is_some());
    assert!(default_ir_mapper(Dialect::Gemini, Dialect::Claude).is_some());
}

#[test]
fn factory_openai_codex() {
    assert!(default_ir_mapper(Dialect::OpenAi, Dialect::Codex).is_some());
    assert!(default_ir_mapper(Dialect::Codex, Dialect::OpenAi).is_some());
}

#[test]
fn factory_openai_kimi() {
    assert!(default_ir_mapper(Dialect::OpenAi, Dialect::Kimi).is_some());
    assert!(default_ir_mapper(Dialect::Kimi, Dialect::OpenAi).is_some());
}

#[test]
fn factory_claude_kimi() {
    assert!(default_ir_mapper(Dialect::Claude, Dialect::Kimi).is_some());
    assert!(default_ir_mapper(Dialect::Kimi, Dialect::Claude).is_some());
}

#[test]
fn factory_openai_copilot() {
    assert!(default_ir_mapper(Dialect::OpenAi, Dialect::Copilot).is_some());
    assert!(default_ir_mapper(Dialect::Copilot, Dialect::OpenAi).is_some());
}

#[test]
fn factory_gemini_kimi() {
    assert!(default_ir_mapper(Dialect::Gemini, Dialect::Kimi).is_some());
    assert!(default_ir_mapper(Dialect::Kimi, Dialect::Gemini).is_some());
}

#[test]
fn factory_codex_claude() {
    assert!(default_ir_mapper(Dialect::Codex, Dialect::Claude).is_some());
    assert!(default_ir_mapper(Dialect::Claude, Dialect::Codex).is_some());
}

#[test]
fn factory_unsupported_pairs_return_none() {
    // Codex ↔ Gemini is not supported
    assert!(default_ir_mapper(Dialect::Codex, Dialect::Gemini).is_none());
    assert!(default_ir_mapper(Dialect::Gemini, Dialect::Codex).is_none());
    // Codex ↔ Kimi
    assert!(default_ir_mapper(Dialect::Codex, Dialect::Kimi).is_none());
    // Copilot ↔ Claude
    assert!(default_ir_mapper(Dialect::Copilot, Dialect::Claude).is_none());
}

#[test]
fn supported_ir_pairs_contains_all_identity() {
    let pairs = supported_ir_pairs();
    for &d in Dialect::all() {
        assert!(pairs.contains(&(d, d)), "missing identity pair for {d}");
    }
}

#[test]
fn supported_ir_pairs_non_empty() {
    let pairs = supported_ir_pairs();
    assert!(pairs.len() >= 20); // 6 identity + 18 cross = 24
}

#[test]
fn all_supported_pairs_resolve() {
    for (from, to) in supported_ir_pairs() {
        assert!(
            default_ir_mapper(from, to).is_some(),
            "supported pair ({from}, {to}) returned None"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §3 OpenAi↔Claude mapper: messages, tool calls, system prompts
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn o2c_basic_user_message() {
    let m = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({"model": "gpt-4", "messages": [{"role": "user", "content": "Hello"}], "max_tokens": 512}),
    };
    let r = m.map_request(&req).unwrap();
    assert_eq!(r["messages"][0]["role"], "user");
    assert_eq!(r["messages"][0]["content"], "Hello");
    assert_eq!(r["max_tokens"], 512);
}

#[test]
fn o2c_system_extracted() {
    let m = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [
                {"role": "system", "content": "Be concise."},
                {"role": "user", "content": "Hi"}
            ]
        }),
    };
    let r = m.map_request(&req).unwrap();
    assert_eq!(r["system"], "Be concise.");
    assert_eq!(r["messages"].as_array().unwrap().len(), 1);
}

#[test]
fn o2c_multiple_systems_joined() {
    let m = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [
                {"role": "system", "content": "A"},
                {"role": "system", "content": "B"},
                {"role": "user", "content": "Go"}
            ]
        }),
    };
    let r = m.map_request(&req).unwrap();
    assert_eq!(r["system"], "A\n\nB");
}

#[test]
fn o2c_default_max_tokens() {
    let m = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({"model": "gpt-4", "messages": [{"role": "user", "content": "x"}]}),
    };
    assert_eq!(m.map_request(&req).unwrap()["max_tokens"], 4096);
}

#[test]
fn o2c_tools_mapped() {
    let m = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "x"}],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "f",
                    "description": "desc",
                    "parameters": {"type": "object", "properties": {"a": {"type": "string"}}}
                }
            }]
        }),
    };
    let r = m.map_request(&req).unwrap();
    assert_eq!(r["tools"][0]["name"], "f");
    assert_eq!(r["tools"][0]["description"], "desc");
    assert!(r["tools"][0]["input_schema"].is_object());
}

#[test]
fn o2c_stop_string_to_array() {
    let m = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({"model": "gpt-4", "messages": [{"role": "user", "content": "x"}], "stop": "END"}),
    };
    assert_eq!(
        m.map_request(&req).unwrap()["stop_sequences"],
        json!(["END"])
    );
}

#[test]
fn o2c_stream_mapped() {
    let m = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({"model": "gpt-4", "messages": [{"role": "user", "content": "x"}], "stream": true}),
    };
    assert_eq!(m.map_request(&req).unwrap()["stream"], true);
}

#[test]
fn o2c_wrong_dialect_rejected() {
    let m = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::Claude,
        body: json!({}),
    };
    assert!(m.map_request(&req).is_err());
}

#[test]
fn o2c_non_object_rejected() {
    let m = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!("str"),
    };
    assert!(m.map_request(&req).is_err());
}

#[test]
fn o2c_response_tagged_as_claude() {
    let m = OpenAiToClaudeMapper;
    let resp = m.map_response(&json!({})).unwrap();
    assert_eq!(resp.dialect, Dialect::Claude);
}

#[test]
fn o2c_source_target() {
    let m = OpenAiToClaudeMapper;
    assert_eq!(m.source_dialect(), Dialect::OpenAi);
    assert_eq!(m.target_dialect(), Dialect::Claude);
}

#[test]
fn o2c_event_assistant_delta() {
    let m = OpenAiToClaudeMapper;
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: "t".into() },
        ext: None,
    };
    let v = m.map_event(&ev).unwrap();
    assert_eq!(v["type"], "content_block_delta");
    assert_eq!(v["delta"]["text"], "t");
}

#[test]
fn o2c_event_tool_call() {
    let m = OpenAiToClaudeMapper;
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: Some("t1".into()),
            parent_tool_use_id: None,
            input: json!({}),
        },
        ext: None,
    };
    let v = m.map_event(&ev).unwrap();
    assert_eq!(v["content_block"]["name"], "bash");
}

#[test]
fn o2c_assistant_with_tool_calls() {
    let m = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [
                {"role": "user", "content": "Weather?"},
                {"role": "assistant", "content": null, "tool_calls": [{
                    "id": "c1", "type": "function",
                    "function": {"name": "weather", "arguments": "{\"city\":\"NY\"}"}
                }]},
                {"role": "tool", "tool_call_id": "c1", "content": "sunny"}
            ]
        }),
    };
    let r = m.map_request(&req).unwrap();
    let msgs = r["messages"].as_array().unwrap();
    assert_eq!(msgs[1]["content"][0]["type"], "tool_use");
    assert_eq!(msgs[1]["content"][0]["name"], "weather");
    assert_eq!(msgs[2]["content"][0]["type"], "tool_result");
}

// Claude → OpenAI

#[test]
fn c2o_basic_user_message() {
    let m = ClaudeToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::Claude,
        body: json!({
            "model": "claude-3", "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        }),
    };
    let r = m.map_request(&req).unwrap();
    assert_eq!(r["messages"][0]["role"], "user");
    assert_eq!(r["messages"][0]["content"], "Hello");
}

#[test]
fn c2o_system_to_system_message() {
    let m = ClaudeToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::Claude,
        body: json!({
            "model": "claude-3", "max_tokens": 1024,
            "system": "Be helpful.",
            "messages": [{"role": "user", "content": "Hi"}]
        }),
    };
    let r = m.map_request(&req).unwrap();
    assert_eq!(r["messages"][0]["role"], "system");
    assert_eq!(r["messages"][0]["content"], "Be helpful.");
}

#[test]
fn c2o_wrong_dialect_rejected() {
    let m = ClaudeToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({}),
    };
    assert!(m.map_request(&req).is_err());
}

#[test]
fn c2o_non_object_rejected() {
    let m = ClaudeToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::Claude,
        body: json!(42),
    };
    assert!(m.map_request(&req).is_err());
}

#[test]
fn c2o_response_tagged_as_openai() {
    let m = ClaudeToOpenAiMapper;
    let resp = m.map_response(&json!({})).unwrap();
    assert_eq!(resp.dialect, Dialect::OpenAi);
}

#[test]
fn c2o_source_target() {
    let m = ClaudeToOpenAiMapper;
    assert_eq!(m.source_dialect(), Dialect::Claude);
    assert_eq!(m.target_dialect(), Dialect::OpenAi);
}

#[test]
fn c2o_event_assistant_delta() {
    let m = ClaudeToOpenAiMapper;
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: "tok".into() },
        ext: None,
    };
    let v = m.map_event(&ev).unwrap();
    assert_eq!(v["object"], "chat.completion.chunk");
    assert_eq!(v["choices"][0]["delta"]["content"], "tok");
}

#[test]
fn c2o_tools_mapped() {
    let m = ClaudeToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::Claude,
        body: json!({
            "model": "claude-3", "max_tokens": 1024,
            "messages": [{"role": "user", "content": "x"}],
            "tools": [{
                "name": "f",
                "description": "desc",
                "input_schema": {"type": "object", "properties": {"a": {"type": "string"}}}
            }]
        }),
    };
    let r = m.map_request(&req).unwrap();
    assert_eq!(r["tools"][0]["type"], "function");
    assert_eq!(r["tools"][0]["function"]["name"], "f");
}

#[test]
fn c2o_assistant_tool_use_blocks() {
    let m = ClaudeToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::Claude,
        body: json!({
            "model": "claude-3", "max_tokens": 1024,
            "messages": [
                {"role": "user", "content": "Do it"},
                {"role": "assistant", "content": [
                    {"type": "text", "text": "Sure"},
                    {"type": "tool_use", "id": "tu1", "name": "bash", "input": {"cmd": "ls"}}
                ]}
            ]
        }),
    };
    let r = m.map_request(&req).unwrap();
    let asst = &r["messages"][1];
    assert_eq!(asst["role"], "assistant");
    assert!(asst["tool_calls"].is_array());
    assert_eq!(asst["tool_calls"][0]["function"]["name"], "bash");
}

// ═══════════════════════════════════════════════════════════════════════════
// §4 OpenAi↔Gemini mapper: messages, content types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn o2g_basic_user_message() {
    let m = OpenAiToGeminiMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({"model": "gpt-4", "messages": [{"role": "user", "content": "Hello"}]}),
    };
    let r = m.map_request(&req).unwrap();
    assert_eq!(r["contents"][0]["parts"][0]["text"], "Hello");
    assert_eq!(r["contents"][0]["role"], "user");
}

#[test]
fn o2g_system_to_system_instruction() {
    let m = OpenAiToGeminiMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [
                {"role": "system", "content": "Help"},
                {"role": "user", "content": "Hi"}
            ]
        }),
    };
    let r = m.map_request(&req).unwrap();
    assert_eq!(r["system_instruction"]["parts"][0]["text"], "Help");
}

#[test]
fn o2g_generation_config() {
    let m = OpenAiToGeminiMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "x"}],
            "max_tokens": 100, "temperature": 0.5, "top_p": 0.8, "stop": ["X"]
        }),
    };
    let r = m.map_request(&req).unwrap();
    assert_eq!(r["generationConfig"]["maxOutputTokens"], 100);
    assert_eq!(r["generationConfig"]["temperature"], 0.5);
    assert_eq!(r["generationConfig"]["topP"], 0.8);
    assert_eq!(r["generationConfig"]["stopSequences"], json!(["X"]));
}

#[test]
fn o2g_tools_to_function_declarations() {
    let m = OpenAiToGeminiMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "x"}],
            "tools": [{"type": "function", "function": {"name": "f", "description": "d", "parameters": {}}}]
        }),
    };
    let r = m.map_request(&req).unwrap();
    assert_eq!(r["tools"][0]["function_declarations"][0]["name"], "f");
}

#[test]
fn o2g_wrong_dialect_rejected() {
    let m = OpenAiToGeminiMapper;
    let req = DialectRequest {
        dialect: Dialect::Claude,
        body: json!({}),
    };
    assert!(m.map_request(&req).is_err());
}

#[test]
fn o2g_response_tagged_as_gemini() {
    let m = OpenAiToGeminiMapper;
    assert_eq!(m.map_response(&json!({})).unwrap().dialect, Dialect::Gemini);
}

#[test]
fn o2g_source_target() {
    let m = OpenAiToGeminiMapper;
    assert_eq!(m.source_dialect(), Dialect::OpenAi);
    assert_eq!(m.target_dialect(), Dialect::Gemini);
}

// Gemini → OpenAI

#[test]
fn g2o_basic_user_message() {
    let m = GeminiToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::Gemini,
        body: json!({
            "model": "gemini-pro",
            "contents": [{"role": "user", "parts": [{"text": "Hello"}]}]
        }),
    };
    let r = m.map_request(&req).unwrap();
    assert_eq!(r["messages"][0]["role"], "user");
    assert_eq!(r["messages"][0]["content"], "Hello");
}

#[test]
fn g2o_system_instruction_becomes_message() {
    let m = GeminiToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::Gemini,
        body: json!({
            "model": "gemini-pro",
            "system_instruction": {"parts": [{"text": "Help"}]},
            "contents": [{"role": "user", "parts": [{"text": "Hi"}]}]
        }),
    };
    let r = m.map_request(&req).unwrap();
    assert_eq!(r["messages"][0]["role"], "system");
    assert_eq!(r["messages"][0]["content"], "Help");
}

#[test]
fn g2o_model_becomes_assistant() {
    let m = GeminiToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::Gemini,
        body: json!({
            "model": "gemini-pro",
            "contents": [
                {"role": "user", "parts": [{"text": "Hi"}]},
                {"role": "model", "parts": [{"text": "Hello!"}]}
            ]
        }),
    };
    let r = m.map_request(&req).unwrap();
    assert_eq!(r["messages"][1]["role"], "assistant");
    assert_eq!(r["messages"][1]["content"], "Hello!");
}

#[test]
fn g2o_generation_config_flattened() {
    let m = GeminiToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::Gemini,
        body: json!({
            "model": "gemini-pro",
            "contents": [{"role": "user", "parts": [{"text": "x"}]}],
            "generationConfig": {"maxOutputTokens": 512, "temperature": 0.3, "topP": 0.7, "stopSequences": ["S"]}
        }),
    };
    let r = m.map_request(&req).unwrap();
    assert_eq!(r["max_tokens"], 512);
    assert_eq!(r["temperature"], 0.3);
    assert_eq!(r["top_p"], 0.7);
    assert_eq!(r["stop"], json!(["S"]));
}

#[test]
fn g2o_wrong_dialect_rejected() {
    let m = GeminiToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({}),
    };
    assert!(m.map_request(&req).is_err());
}

#[test]
fn g2o_source_target() {
    let m = GeminiToOpenAiMapper;
    assert_eq!(m.source_dialect(), Dialect::Gemini);
    assert_eq!(m.target_dialect(), Dialect::OpenAi);
}

#[test]
fn g2o_function_call_in_model() {
    let m = GeminiToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::Gemini,
        body: json!({
            "model": "gemini-pro",
            "contents": [
                {"role": "user", "parts": [{"text": "?"}]},
                {"role": "model", "parts": [{"functionCall": {"name": "f", "args": {"x": 1}}}]}
            ]
        }),
    };
    let r = m.map_request(&req).unwrap();
    assert!(r["messages"][1]["tool_calls"].is_array());
    assert_eq!(r["messages"][1]["tool_calls"][0]["function"]["name"], "f");
}

#[test]
fn g2o_function_response_becomes_tool() {
    let m = GeminiToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::Gemini,
        body: json!({
            "model": "gemini-pro",
            "contents": [{"role": "user", "parts": [{"functionResponse": {"name": "f", "response": "ok"}}]}]
        }),
    };
    let r = m.map_request(&req).unwrap();
    assert_eq!(r["messages"][0]["role"], "tool");
}

// ═══════════════════════════════════════════════════════════════════════════
// §5 IR mappers: roundtrip consistency, error cases
// ═══════════════════════════════════════════════════════════════════════════

fn simple_ir() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "System prompt."),
        IrMessage::text(IrRole::User, "Hello"),
        IrMessage::text(IrRole::Assistant, "Hi!"),
    ])
}

fn tool_call_ir() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Weather?"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Checking.".into(),
                },
                IrContentBlock::ToolUse {
                    id: "c1".into(),
                    name: "weather".into(),
                    input: json!({"city": "NY"}),
                },
            ],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "c1".into(),
                content: vec![IrContentBlock::Text {
                    text: "sunny".into(),
                }],
                is_error: false,
            }],
        ),
    ])
}

fn thinking_ir() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Think about this"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "hmm...".into(),
                },
                IrContentBlock::Text {
                    text: "Answer".into(),
                },
            ],
        ),
    ])
}

// IrIdentityMapper

#[test]
fn ir_identity_request_passthrough() {
    let m = IrIdentityMapper;
    let ir = simple_ir();
    let result = m
        .map_request(Dialect::OpenAi, Dialect::OpenAi, &ir)
        .unwrap();
    assert_eq!(result, ir);
}

#[test]
fn ir_identity_response_passthrough() {
    let m = IrIdentityMapper;
    let ir = simple_ir();
    let result = m
        .map_response(Dialect::Claude, Dialect::Claude, &ir)
        .unwrap();
    assert_eq!(result, ir);
}

#[test]
fn ir_identity_supported_pairs() {
    let m = IrIdentityMapper;
    let pairs = m.supported_pairs();
    for &d in Dialect::all() {
        assert!(pairs.contains(&(d, d)));
    }
}

// OpenAiClaudeIrMapper

#[test]
fn ir_openai_claude_simple() {
    let m = OpenAiClaudeIrMapper;
    let ir = simple_ir();
    let mapped = m
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    assert_eq!(mapped.messages.len(), 3);
    assert_eq!(mapped.messages[0].role, IrRole::System);
}

#[test]
fn ir_openai_claude_tool_role_becomes_user() {
    let m = OpenAiClaudeIrMapper;
    let ir = tool_call_ir();
    let mapped = m
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    // The Tool message should become User in Claude
    let last = mapped.messages.last().unwrap();
    assert_eq!(last.role, IrRole::User);
}

#[test]
fn ir_claude_openai_drops_thinking() {
    let m = OpenAiClaudeIrMapper;
    let ir = thinking_ir();
    let mapped = m
        .map_request(Dialect::Claude, Dialect::OpenAi, &ir)
        .unwrap();
    let asst = &mapped.messages[1];
    assert!(
        asst.content
            .iter()
            .all(|b| !matches!(b, IrContentBlock::Thinking { .. }))
    );
    assert_eq!(asst.content.len(), 1); // only Text
}

#[test]
fn ir_openai_claude_preserves_thinking() {
    let m = OpenAiClaudeIrMapper;
    let ir = thinking_ir();
    let mapped = m
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    let asst = &mapped.messages[1];
    assert!(
        asst.content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
    );
}

#[test]
fn ir_openai_claude_unsupported_pair() {
    let m = OpenAiClaudeIrMapper;
    let ir = simple_ir();
    let err = m
        .map_request(Dialect::Gemini, Dialect::Kimi, &ir)
        .unwrap_err();
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
}

#[test]
fn ir_openai_claude_supported_pairs() {
    let m = OpenAiClaudeIrMapper;
    let pairs = m.supported_pairs();
    assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Claude)));
    assert!(pairs.contains(&(Dialect::Claude, Dialect::OpenAi)));
}

// OpenAiGeminiIrMapper

#[test]
fn ir_openai_gemini_simple() {
    let m = OpenAiGeminiIrMapper;
    let ir = simple_ir();
    let mapped = m
        .map_request(Dialect::OpenAi, Dialect::Gemini, &ir)
        .unwrap();
    assert_eq!(mapped.messages[0].role, IrRole::System);
}

#[test]
fn ir_openai_gemini_tool_becomes_user() {
    let m = OpenAiGeminiIrMapper;
    let ir = tool_call_ir();
    let mapped = m
        .map_request(Dialect::OpenAi, Dialect::Gemini, &ir)
        .unwrap();
    let last = mapped.messages.last().unwrap();
    assert_eq!(last.role, IrRole::User);
}

#[test]
fn ir_openai_gemini_drops_thinking() {
    let m = OpenAiGeminiIrMapper;
    let ir = thinking_ir();
    let mapped = m
        .map_request(Dialect::OpenAi, Dialect::Gemini, &ir)
        .unwrap();
    for msg in &mapped.messages {
        assert!(
            !msg.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
        );
    }
}

#[test]
fn ir_gemini_openai_user_tool_results_become_tool_role() {
    let m = OpenAiGeminiIrMapper;
    // Gemini pattern: user message with only ToolResult blocks
    let ir = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "q"),
        IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "c1".into(),
                content: vec![IrContentBlock::Text { text: "ok".into() }],
                is_error: false,
            }],
        ),
    ]);
    let mapped = m
        .map_request(Dialect::Gemini, Dialect::OpenAi, &ir)
        .unwrap();
    assert!(mapped.messages.iter().any(|m| m.role == IrRole::Tool));
}

#[test]
fn ir_openai_gemini_unsupported_pair() {
    let m = OpenAiGeminiIrMapper;
    let err = m
        .map_request(Dialect::Claude, Dialect::Kimi, &simple_ir())
        .unwrap_err();
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
}

// ClaudeGeminiIrMapper

#[test]
fn ir_claude_gemini_simple() {
    let m = ClaudeGeminiIrMapper;
    let ir = simple_ir();
    let mapped = m
        .map_request(Dialect::Claude, Dialect::Gemini, &ir)
        .unwrap();
    assert_eq!(mapped.messages.len(), 3);
}

#[test]
fn ir_claude_gemini_drops_thinking() {
    let m = ClaudeGeminiIrMapper;
    let mapped = m
        .map_request(Dialect::Claude, Dialect::Gemini, &thinking_ir())
        .unwrap();
    for msg in &mapped.messages {
        assert!(
            !msg.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
        );
    }
}

#[test]
fn ir_claude_gemini_system_image_fails() {
    let m = ClaudeGeminiIrMapper;
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::System,
        vec![IrContentBlock::Image {
            media_type: "image/png".into(),
            data: "abc".into(),
        }],
    )]);
    let err = m
        .map_request(Dialect::Claude, Dialect::Gemini, &ir)
        .unwrap_err();
    assert!(matches!(err, MapError::UnmappableContent { .. }));
}

#[test]
fn ir_gemini_claude_tool_becomes_user() {
    let m = ClaudeGeminiIrMapper;
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "c1".into(),
            content: vec![],
            is_error: false,
        }],
    )]);
    let mapped = m
        .map_request(Dialect::Gemini, Dialect::Claude, &ir)
        .unwrap();
    assert_eq!(mapped.messages[0].role, IrRole::User);
}

#[test]
fn ir_claude_gemini_unsupported_pair() {
    let m = ClaudeGeminiIrMapper;
    let err = m
        .map_request(Dialect::OpenAi, Dialect::Kimi, &simple_ir())
        .unwrap_err();
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
}

// OpenAiCodexIrMapper (lossy)

#[test]
fn ir_openai_codex_drops_system() {
    let m = OpenAiCodexIrMapper;
    let ir = simple_ir();
    let mapped = m.map_request(Dialect::OpenAi, Dialect::Codex, &ir).unwrap();
    assert!(!mapped.messages.iter().any(|m| m.role == IrRole::System));
}

#[test]
fn ir_openai_codex_drops_tool_role() {
    let m = OpenAiCodexIrMapper;
    let ir = tool_call_ir();
    let mapped = m.map_request(Dialect::OpenAi, Dialect::Codex, &ir).unwrap();
    assert!(!mapped.messages.iter().any(|m| m.role == IrRole::Tool));
}

#[test]
fn ir_openai_codex_strips_tool_blocks() {
    let m = OpenAiCodexIrMapper;
    let ir = tool_call_ir();
    let mapped = m.map_request(Dialect::OpenAi, Dialect::Codex, &ir).unwrap();
    for msg in &mapped.messages {
        for block in &msg.content {
            assert!(matches!(block, IrContentBlock::Text { .. }));
        }
    }
}

#[test]
fn ir_codex_openai_is_lossless() {
    let m = OpenAiCodexIrMapper;
    let ir = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "hi"),
        IrMessage::text(IrRole::Assistant, "hello"),
    ]);
    let mapped = m.map_request(Dialect::Codex, Dialect::OpenAi, &ir).unwrap();
    assert_eq!(mapped, ir);
}

#[test]
fn ir_openai_codex_unsupported_pair() {
    let m = OpenAiCodexIrMapper;
    let err = m
        .map_request(Dialect::Claude, Dialect::Gemini, &simple_ir())
        .unwrap_err();
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
}

// OpenAiKimiIrMapper (near-identity)

#[test]
fn ir_openai_kimi_strips_thinking() {
    let m = OpenAiKimiIrMapper;
    let ir = thinking_ir();
    let mapped = m.map_request(Dialect::OpenAi, Dialect::Kimi, &ir).unwrap();
    for msg in &mapped.messages {
        assert!(
            !msg.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
        );
    }
}

#[test]
fn ir_kimi_openai_strips_thinking() {
    let m = OpenAiKimiIrMapper;
    let ir = thinking_ir();
    let mapped = m.map_request(Dialect::Kimi, Dialect::OpenAi, &ir).unwrap();
    for msg in &mapped.messages {
        assert!(
            !msg.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
        );
    }
}

#[test]
fn ir_openai_kimi_preserves_text() {
    let m = OpenAiKimiIrMapper;
    let ir = simple_ir();
    let mapped = m.map_request(Dialect::OpenAi, Dialect::Kimi, &ir).unwrap();
    assert_eq!(mapped.messages.len(), 3);
}

#[test]
fn ir_openai_kimi_unsupported_pair() {
    let m = OpenAiKimiIrMapper;
    let err = m
        .map_request(Dialect::Claude, Dialect::Gemini, &simple_ir())
        .unwrap_err();
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
}

// OpenAiCopilotIrMapper (near-identity)

#[test]
fn ir_openai_copilot_strips_thinking() {
    let m = OpenAiCopilotIrMapper;
    let ir = thinking_ir();
    let mapped = m
        .map_request(Dialect::OpenAi, Dialect::Copilot, &ir)
        .unwrap();
    for msg in &mapped.messages {
        assert!(
            !msg.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
        );
    }
}

#[test]
fn ir_copilot_openai_strips_thinking() {
    let m = OpenAiCopilotIrMapper;
    let mapped = m
        .map_request(Dialect::Copilot, Dialect::OpenAi, &thinking_ir())
        .unwrap();
    for msg in &mapped.messages {
        assert!(
            !msg.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
        );
    }
}

#[test]
fn ir_openai_copilot_unsupported_pair() {
    let m = OpenAiCopilotIrMapper;
    let err = m
        .map_request(Dialect::Gemini, Dialect::Claude, &simple_ir())
        .unwrap_err();
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
}

// ClaudeKimiIrMapper

#[test]
fn ir_claude_kimi_tool_result_becomes_tool_role() {
    let m = ClaudeKimiIrMapper;
    // Claude style: User message with ToolResult blocks
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "c1".into(),
            content: vec![],
            is_error: false,
        }],
    )]);
    let mapped = m.map_request(Dialect::Claude, Dialect::Kimi, &ir).unwrap();
    assert_eq!(mapped.messages[0].role, IrRole::Tool);
}

#[test]
fn ir_kimi_claude_tool_becomes_user() {
    let m = ClaudeKimiIrMapper;
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "c1".into(),
            content: vec![],
            is_error: false,
        }],
    )]);
    let mapped = m.map_request(Dialect::Kimi, Dialect::Claude, &ir).unwrap();
    assert_eq!(mapped.messages[0].role, IrRole::User);
}

#[test]
fn ir_claude_kimi_drops_thinking() {
    let m = ClaudeKimiIrMapper;
    let mapped = m
        .map_request(Dialect::Claude, Dialect::Kimi, &thinking_ir())
        .unwrap();
    for msg in &mapped.messages {
        assert!(
            !msg.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
        );
    }
}

#[test]
fn ir_claude_kimi_unsupported_pair() {
    let m = ClaudeKimiIrMapper;
    let err = m
        .map_request(Dialect::OpenAi, Dialect::Gemini, &simple_ir())
        .unwrap_err();
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
}

// GeminiKimiIrMapper

#[test]
fn ir_gemini_kimi_tool_result_becomes_tool_role() {
    let m = GeminiKimiIrMapper;
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "c1".into(),
            content: vec![],
            is_error: false,
        }],
    )]);
    let mapped = m.map_request(Dialect::Gemini, Dialect::Kimi, &ir).unwrap();
    assert_eq!(mapped.messages[0].role, IrRole::Tool);
}

#[test]
fn ir_kimi_gemini_tool_becomes_user() {
    let m = GeminiKimiIrMapper;
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "c1".into(),
            content: vec![],
            is_error: false,
        }],
    )]);
    let mapped = m.map_request(Dialect::Kimi, Dialect::Gemini, &ir).unwrap();
    assert_eq!(mapped.messages[0].role, IrRole::User);
}

#[test]
fn ir_gemini_kimi_drops_thinking() {
    let m = GeminiKimiIrMapper;
    let mapped = m
        .map_request(Dialect::Gemini, Dialect::Kimi, &thinking_ir())
        .unwrap();
    for msg in &mapped.messages {
        assert!(
            !msg.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
        );
    }
}

#[test]
fn ir_gemini_kimi_unsupported_pair() {
    let m = GeminiKimiIrMapper;
    let err = m
        .map_request(Dialect::OpenAi, Dialect::Claude, &simple_ir())
        .unwrap_err();
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
}

// CodexClaudeIrMapper (lossy)

#[test]
fn ir_claude_codex_drops_system_and_tools() {
    let m = CodexClaudeIrMapper;
    let ir = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "sys"),
        IrMessage::text(IrRole::User, "hi"),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "c1".into(),
                content: vec![],
                is_error: false,
            }],
        ),
    ]);
    let mapped = m.map_request(Dialect::Claude, Dialect::Codex, &ir).unwrap();
    assert!(!mapped.messages.iter().any(|m| m.role == IrRole::System));
    assert!(!mapped.messages.iter().any(|m| m.role == IrRole::Tool));
}

#[test]
fn ir_codex_claude_lossless() {
    let m = CodexClaudeIrMapper;
    let ir = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "hi"),
        IrMessage::text(IrRole::Assistant, "hello"),
    ]);
    let mapped = m.map_request(Dialect::Codex, Dialect::Claude, &ir).unwrap();
    assert_eq!(mapped, ir);
}

#[test]
fn ir_codex_claude_unmappable_tool() {
    let m = CodexClaudeIrMapper;
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "t1".into(),
            name: "apply_patch".into(),
            input: json!({}),
        }],
    )]);
    let err = m
        .map_request(Dialect::Codex, Dialect::Claude, &ir)
        .unwrap_err();
    assert!(matches!(err, MapError::UnmappableTool { .. }));
}

#[test]
fn ir_codex_claude_apply_diff_rejected() {
    let m = CodexClaudeIrMapper;
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "t1".into(),
            name: "apply_diff".into(),
            input: json!({}),
        }],
    )]);
    let err = m
        .map_request(Dialect::Codex, Dialect::Claude, &ir)
        .unwrap_err();
    assert!(matches!(err, MapError::UnmappableTool { .. }));
}

#[test]
fn ir_codex_claude_unsupported_pair() {
    let m = CodexClaudeIrMapper;
    let err = m
        .map_request(Dialect::OpenAi, Dialect::Gemini, &simple_ir())
        .unwrap_err();
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
}

// ═══════════════════════════════════════════════════════════════════════════
// §6 MapError variants: construction, Display, serialization
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn map_error_unsupported_pair_display() {
    let err = MapError::UnsupportedPair {
        from: Dialect::Kimi,
        to: Dialect::Copilot,
    };
    let s = err.to_string();
    assert!(s.contains("Kimi"));
    assert!(s.contains("Copilot"));
}

#[test]
fn map_error_lossy_conversion_display() {
    let err = MapError::LossyConversion {
        field: "thinking".into(),
        reason: "dropped".into(),
    };
    let s = err.to_string();
    assert!(s.contains("thinking"));
    assert!(s.contains("lossy"));
}

#[test]
fn map_error_unmappable_tool_display() {
    let err = MapError::UnmappableTool {
        name: "bash".into(),
        reason: "nope".into(),
    };
    assert!(err.to_string().contains("bash"));
}

#[test]
fn map_error_incompatible_capability_display() {
    let err = MapError::IncompatibleCapability {
        capability: "logprobs".into(),
        reason: "no support".into(),
    };
    assert!(err.to_string().contains("logprobs"));
}

#[test]
fn map_error_unmappable_content_display() {
    let err = MapError::UnmappableContent {
        field: "system".into(),
        reason: "images".into(),
    };
    assert!(err.to_string().contains("system"));
}

#[test]
fn map_error_serde_roundtrip_all_variants() {
    let variants: Vec<MapError> = vec![
        MapError::UnsupportedPair {
            from: Dialect::OpenAi,
            to: Dialect::Claude,
        },
        MapError::LossyConversion {
            field: "f".into(),
            reason: "r".into(),
        },
        MapError::UnmappableTool {
            name: "n".into(),
            reason: "r".into(),
        },
        MapError::IncompatibleCapability {
            capability: "c".into(),
            reason: "r".into(),
        },
        MapError::UnmappableContent {
            field: "f".into(),
            reason: "r".into(),
        },
    ];
    for err in &variants {
        let json = serde_json::to_string(err).unwrap();
        let back: MapError = serde_json::from_str(&json).unwrap();
        assert_eq!(*err, back);
    }
}

#[test]
fn map_error_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MapError>();
}

#[test]
fn map_error_clone() {
    let err = MapError::UnsupportedPair {
        from: Dialect::Gemini,
        to: Dialect::Kimi,
    };
    assert_eq!(err, err.clone());
}

// MappingError (the JSON-mapper error type)

#[test]
fn mapping_error_unsupported_capability() {
    let err = MappingError::UnsupportedCapability {
        capability: "logprobs".into(),
        source_dialect: Dialect::Claude,
        target_dialect: Dialect::OpenAi,
    };
    assert!(err.to_string().contains("logprobs"));
}

#[test]
fn mapping_error_incompatible_types() {
    let err = MappingError::IncompatibleTypes {
        source_type: "a".into(),
        target_type: "b".into(),
        reason: "mismatch".into(),
    };
    assert!(err.to_string().contains("mismatch"));
}

#[test]
fn mapping_error_fidelity_loss() {
    let err = MappingError::FidelityLoss {
        field: "thinking".into(),
        source_dialect: Dialect::Claude,
        target_dialect: Dialect::OpenAi,
        detail: "no block".into(),
    };
    assert!(err.to_string().contains("fidelity loss"));
}

#[test]
fn mapping_error_unmappable_request() {
    let err = MappingError::UnmappableRequest {
        reason: "empty".into(),
    };
    assert!(err.to_string().contains("empty"));
}

#[test]
fn mapping_error_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MappingError>();
}

#[test]
fn mapping_error_clone() {
    let err = MappingError::UnsupportedCapability {
        capability: "x".into(),
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
    };
    assert_eq!(err.to_string(), err.clone().to_string());
}

// ═══════════════════════════════════════════════════════════════════════════
// §7 Validation: valid/invalid mappings
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn validation_valid_openai() {
    let v = DefaultMappingValidator::new();
    let req = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let r = v.validate_pre_mapping(Dialect::OpenAi, &req);
    assert!(r.is_valid());
    assert_eq!(r.field_coverage, 100.0);
}

#[test]
fn validation_missing_model() {
    let v = DefaultMappingValidator::new();
    let req = json!({"messages": []});
    let r = v.validate_pre_mapping(Dialect::OpenAi, &req);
    assert!(!r.is_valid());
    assert!(r.error_count() > 0);
}

#[test]
fn validation_non_object() {
    let v = DefaultMappingValidator::new();
    let r = v.validate_pre_mapping(Dialect::OpenAi, &json!("string"));
    assert!(!r.is_valid());
    assert_eq!(r.field_coverage, 0.0);
}

#[test]
fn validation_claude_requires_max_tokens() {
    let v = DefaultMappingValidator::new();
    let req = json!({"model": "claude-3", "messages": []});
    let r = v.validate_pre_mapping(Dialect::Claude, &req);
    assert!(!r.is_valid());
    assert!(r.issues.iter().any(|i| i.field == "max_tokens"));
}

#[test]
fn validation_gemini_requires_contents() {
    let v = DefaultMappingValidator::new();
    let req = json!({"model": "gemini-pro"});
    let r = v.validate_pre_mapping(Dialect::Gemini, &req);
    assert!(!r.is_valid());
    assert!(r.issues.iter().any(|i| i.field == "contents"));
}

#[test]
fn validation_post_mapping_valid() {
    let v = DefaultMappingValidator::new();
    let mapped = json!({"model": "claude-3", "messages": [{}], "max_tokens": 1024});
    assert!(v.validate_post_mapping(Dialect::Claude, &mapped).is_valid());
}

#[test]
fn validation_post_mapping_missing() {
    let v = DefaultMappingValidator::new();
    let mapped = json!({"model": "gpt-4"});
    assert!(!v.validate_post_mapping(Dialect::OpenAi, &mapped).is_valid());
}

#[test]
fn validation_empty_messages_warning() {
    let v = DefaultMappingValidator::new();
    let req = json!({"model": "gpt-4", "messages": []});
    let r = v.validate_pre_mapping(Dialect::OpenAi, &req);
    assert!(r.is_valid()); // warnings don't fail
    assert!(r.warning_count() > 0);
}

#[test]
fn validation_roundtrip_identical() {
    let v = DefaultMappingValidator::new();
    let val = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let r = v.validate_roundtrip(&val, &val);
    assert!(r.is_lossless());
}

#[test]
fn validation_roundtrip_lost_field() {
    let v = DefaultMappingValidator::new();
    let orig = json!({"model": "gpt-4", "temp": 0.7});
    let rt = json!({"model": "gpt-4"});
    let r = v.validate_roundtrip(&orig, &rt);
    assert!(!r.is_lossless());
    assert!(r.lost_fields.contains(&"temp".to_string()));
}

#[test]
fn validation_roundtrip_added_field() {
    let v = DefaultMappingValidator::new();
    let orig = json!({"model": "gpt-4"});
    let rt = json!({"model": "gpt-4", "extra": true});
    let r = v.validate_roundtrip(&orig, &rt);
    assert!(!r.is_lossless());
    assert!(r.added_fields.contains(&"extra".to_string()));
}

#[test]
fn validation_roundtrip_changed_field() {
    let v = DefaultMappingValidator::new();
    let orig = json!({"v": 1});
    let rt = json!({"v": 2});
    let r = v.validate_roundtrip(&orig, &rt);
    assert!(!r.is_lossless());
    assert!(r.changed_fields.contains(&"v".to_string()));
}

#[test]
fn validation_severity_display() {
    assert_eq!(ValidationSeverity::Info.to_string(), "info");
    assert_eq!(ValidationSeverity::Warning.to_string(), "warning");
    assert_eq!(ValidationSeverity::Error.to_string(), "error");
}

#[test]
fn validation_severity_ordering() {
    assert!(ValidationSeverity::Info < ValidationSeverity::Warning);
    assert!(ValidationSeverity::Warning < ValidationSeverity::Error);
}

// ValidationPipeline

#[test]
fn pipeline_pass_through() {
    let p = ValidationPipeline::new(
        DefaultMappingValidator::new(),
        Dialect::OpenAi,
        Dialect::OpenAi,
    );
    let req = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let r = p.run(&req, |v| Ok(v.clone()));
    assert!(r.pre.is_valid());
    assert!(r.mapped.is_some());
    assert!(r.post.as_ref().unwrap().is_valid());
}

#[test]
fn pipeline_pre_fail_skips_map() {
    let p = ValidationPipeline::new(
        DefaultMappingValidator::new(),
        Dialect::OpenAi,
        Dialect::Claude,
    );
    let req = json!({"broken": true});
    let r = p.run(&req, |_| panic!("should not be called"));
    assert!(!r.pre.is_valid());
    assert!(r.mapped.is_none());
}

#[test]
fn pipeline_map_error_reported() {
    let p = ValidationPipeline::new(
        DefaultMappingValidator::new(),
        Dialect::OpenAi,
        Dialect::Claude,
    );
    let req = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let r = p.run(&req, |_| Err("boom".into()));
    assert!(r.pre.is_valid());
    assert!(r.mapped.is_none());
    assert!(!r.post.unwrap().is_valid());
}

#[test]
fn pipeline_fields_accessible() {
    let p = ValidationPipeline::new(
        DefaultMappingValidator::new(),
        Dialect::OpenAi,
        Dialect::Claude,
    );
    let _ = p.validator();
    assert_eq!(p.source, Dialect::OpenAi);
    assert_eq!(p.target, Dialect::Claude);
}

// ═══════════════════════════════════════════════════════════════════════════
// §8 Edge cases: empty, unicode, nested, large payloads
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn edge_empty_messages_array() {
    let m = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({"model": "gpt-4", "messages": []}),
    };
    let r = m.map_request(&req).unwrap();
    assert_eq!(r["messages"].as_array().unwrap().len(), 0);
}

#[test]
fn edge_unicode_content() {
    let m = IdentityMapper;
    let body =
        json!({"model": "gpt-4", "messages": [{"role": "user", "content": "こんにちは 🦀 Ñ"}]});
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: body.clone(),
    };
    assert_eq!(m.map_request(&req).unwrap(), body);
}

#[test]
fn edge_unicode_in_openai_to_claude() {
    let m = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({"model": "gpt-4", "messages": [{"role": "user", "content": "日本語テスト 🎉"}]}),
    };
    let r = m.map_request(&req).unwrap();
    assert_eq!(r["messages"][0]["content"], "日本語テスト 🎉");
}

#[test]
fn edge_deeply_nested_content() {
    let m = IdentityMapper;
    let nested = json!({
        "a": {"b": {"c": {"d": {"e": {"f": {"g": "deep"}}}}}}
    });
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: nested.clone(),
    };
    assert_eq!(m.map_request(&req).unwrap(), nested);
}

#[test]
fn edge_large_messages_array() {
    let m = OpenAiToClaudeMapper;
    let msgs: Vec<Value> = (0..100)
        .map(|i| json!({"role": "user", "content": format!("Message {i}")}))
        .collect();
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({"model": "gpt-4", "messages": msgs}),
    };
    let r = m.map_request(&req).unwrap();
    assert_eq!(r["messages"].as_array().unwrap().len(), 100);
}

#[test]
fn edge_empty_ir_conversation() {
    let m = IrIdentityMapper;
    let ir = IrConversation::from_messages(vec![]);
    let mapped = m
        .map_request(Dialect::OpenAi, Dialect::OpenAi, &ir)
        .unwrap();
    assert!(mapped.messages.is_empty());
}

#[test]
fn edge_ir_message_with_empty_content() {
    let m = OpenAiClaudeIrMapper;
    let ir = IrConversation::from_messages(vec![IrMessage::new(IrRole::User, vec![])]);
    let mapped = m
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    assert_eq!(mapped.messages.len(), 1);
    assert!(mapped.messages[0].content.is_empty());
}

#[test]
fn edge_multimodal_content_openai_to_gemini() {
    let m = OpenAiToGeminiMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "Look at this"},
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,abc123"}}
                ]
            }]
        }),
    };
    let r = m.map_request(&req).unwrap();
    let parts = r["contents"][0]["parts"].as_array().unwrap();
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0]["text"], "Look at this");
    assert!(parts[1]["inlineData"].is_object());
}

// ═══════════════════════════════════════════════════════════════════════════
// §9 Error propagation when features are unsupported
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_prop_wrong_source_dialect_openai_to_claude() {
    let m = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::Gemini,
        body: json!({"model": "x"}),
    };
    let err = m.map_request(&req).unwrap_err();
    match err {
        MappingError::UnmappableRequest { reason } => assert!(reason.contains("OpenAI")),
        _ => panic!("wrong error variant"),
    }
}

#[test]
fn error_prop_wrong_source_dialect_openai_to_gemini() {
    let m = OpenAiToGeminiMapper;
    let req = DialectRequest {
        dialect: Dialect::Kimi,
        body: json!({"model": "x"}),
    };
    let err = m.map_request(&req).unwrap_err();
    match err {
        MappingError::UnmappableRequest { reason } => assert!(reason.contains("OpenAI")),
        _ => panic!("wrong error variant"),
    }
}

#[test]
fn error_prop_wrong_source_dialect_gemini_to_openai() {
    let m = GeminiToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::Claude,
        body: json!({"model": "x"}),
    };
    assert!(matches!(
        m.map_request(&req).unwrap_err(),
        MappingError::UnmappableRequest { .. }
    ));
}

#[test]
fn error_prop_wrong_source_dialect_claude_to_openai() {
    let m = ClaudeToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::Gemini,
        body: json!({"model": "x"}),
    };
    assert!(matches!(
        m.map_request(&req).unwrap_err(),
        MappingError::UnmappableRequest { .. }
    ));
}

#[test]
fn error_prop_unknown_openai_role() {
    let m = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({"model": "x", "messages": [{"role": "alien", "content": "hi"}]}),
    };
    assert!(matches!(
        m.map_request(&req).unwrap_err(),
        MappingError::IncompatibleTypes { .. }
    ));
}

#[test]
fn error_prop_unknown_gemini_role() {
    let m = GeminiToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::Gemini,
        body: json!({"model": "x", "contents": [{"role": "alien", "parts": []}]}),
    };
    assert!(matches!(
        m.map_request(&req).unwrap_err(),
        MappingError::IncompatibleTypes { .. }
    ));
}

#[test]
fn error_prop_tool_missing_function_field() {
    let m = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({"model": "x", "messages": [{"role": "user", "content": "x"}], "tools": [{"type": "function"}]}),
    };
    assert!(matches!(
        m.map_request(&req).unwrap_err(),
        MappingError::IncompatibleTypes { .. }
    ));
}

#[test]
fn error_prop_ir_unsupported_pair_propagated() {
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
    let err = mapper
        .map_request(Dialect::Gemini, Dialect::Kimi, &simple_ir())
        .unwrap_err();
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
}

// ═══════════════════════════════════════════════════════════════════════════
// §10 Deterministic behaviour (same input → same output)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn deterministic_identity_mapper() {
    let m = IdentityMapper;
    let body = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "test"}]});
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body,
    };
    let a = m.map_request(&req).unwrap();
    let b = m.map_request(&req).unwrap();
    assert_eq!(a, b);
}

#[test]
fn deterministic_openai_to_claude() {
    let m = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4", "max_tokens": 512,
            "messages": [
                {"role": "system", "content": "sys"},
                {"role": "user", "content": "hi"},
                {"role": "assistant", "content": "hello"}
            ],
            "temperature": 0.5
        }),
    };
    let a = m.map_request(&req).unwrap();
    let b = m.map_request(&req).unwrap();
    assert_eq!(a, b);
}

#[test]
fn deterministic_openai_to_gemini() {
    let m = OpenAiToGeminiMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "test"}],
            "max_tokens": 100, "temperature": 0.3
        }),
    };
    let a = m.map_request(&req).unwrap();
    let b = m.map_request(&req).unwrap();
    assert_eq!(a, b);
}

#[test]
fn deterministic_ir_openai_claude() {
    let m = OpenAiClaudeIrMapper;
    let ir = tool_call_ir();
    let a = m
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    let b = m
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    assert_eq!(a, b);
}

#[test]
fn deterministic_ir_openai_gemini() {
    let m = OpenAiGeminiIrMapper;
    let ir = thinking_ir();
    let a = m
        .map_request(Dialect::OpenAi, Dialect::Gemini, &ir)
        .unwrap();
    let b = m
        .map_request(Dialect::OpenAi, Dialect::Gemini, &ir)
        .unwrap();
    assert_eq!(a, b);
}

#[test]
fn deterministic_factory_resolution() {
    for (from, to) in supported_ir_pairs() {
        let a = default_ir_mapper(from, to);
        let b = default_ir_mapper(from, to);
        assert_eq!(a.is_some(), b.is_some());
    }
}

#[test]
fn deterministic_validation() {
    let v = DefaultMappingValidator::new();
    let req = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let a = v.validate_pre_mapping(Dialect::OpenAi, &req);
    let b = v.validate_pre_mapping(Dialect::OpenAi, &req);
    assert_eq!(a.valid, b.valid);
    assert_eq!(a.field_coverage, b.field_coverage);
    assert_eq!(a.issues.len(), b.issues.len());
}

// ═══════════════════════════════════════════════════════════════════════════
// Additional coverage: Mapper trait object safety & misc
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mapper_trait_is_object_safe() {
    let _: Box<dyn Mapper> = Box::new(IdentityMapper);
    let _: Box<dyn Mapper> = Box::new(OpenAiToClaudeMapper);
    let _: Box<dyn Mapper> = Box::new(ClaudeToOpenAiMapper);
    let _: Box<dyn Mapper> = Box::new(OpenAiToGeminiMapper);
    let _: Box<dyn Mapper> = Box::new(GeminiToOpenAiMapper);
}

#[test]
fn ir_mapper_trait_is_object_safe() {
    let _: Box<dyn IrMapper> = Box::new(IrIdentityMapper);
    let _: Box<dyn IrMapper> = Box::new(OpenAiClaudeIrMapper);
    let _: Box<dyn IrMapper> = Box::new(OpenAiGeminiIrMapper);
    let _: Box<dyn IrMapper> = Box::new(ClaudeGeminiIrMapper);
    let _: Box<dyn IrMapper> = Box::new(OpenAiCodexIrMapper);
    let _: Box<dyn IrMapper> = Box::new(OpenAiKimiIrMapper);
    let _: Box<dyn IrMapper> = Box::new(ClaudeKimiIrMapper);
    let _: Box<dyn IrMapper> = Box::new(OpenAiCopilotIrMapper);
    let _: Box<dyn IrMapper> = Box::new(GeminiKimiIrMapper);
    let _: Box<dyn IrMapper> = Box::new(CodexClaudeIrMapper);
}

#[test]
fn dialect_request_clone_and_debug() {
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({"x": 1}),
    };
    let c = req.clone();
    assert_eq!(c.body, req.body);
    let dbg = format!("{req:?}");
    assert!(dbg.contains("OpenAi"));
}

#[test]
fn dialect_response_clone_and_debug() {
    let resp = DialectResponse {
        dialect: Dialect::Claude,
        body: json!({"y": 2}),
    };
    let c = resp.clone();
    assert_eq!(c.body, resp.body);
    let dbg = format!("{resp:?}");
    assert!(dbg.contains("Claude"));
}

#[test]
fn validation_all_dialects_have_required_fields() {
    let v = DefaultMappingValidator::new();
    for &d in Dialect::all() {
        // An empty object should fail for every dialect
        let r = v.validate_pre_mapping(d, &json!({}));
        assert!(!r.is_valid(), "empty object should fail for {d}");
    }
}

#[test]
fn validation_codex_requires_model_and_messages() {
    let v = DefaultMappingValidator::new();
    let req = json!({"model": "codex", "messages": [{"role": "user", "content": "x"}]});
    assert!(v.validate_pre_mapping(Dialect::Codex, &req).is_valid());
}

#[test]
fn validation_kimi_requires_model_and_messages() {
    let v = DefaultMappingValidator::new();
    let req = json!({"model": "kimi", "messages": [{"role": "user", "content": "x"}]});
    assert!(v.validate_pre_mapping(Dialect::Kimi, &req).is_valid());
}

#[test]
fn validation_copilot_requires_model_and_messages() {
    let v = DefaultMappingValidator::new();
    let req = json!({"model": "copilot", "messages": [{"role": "user", "content": "x"}]});
    assert!(v.validate_pre_mapping(Dialect::Copilot, &req).is_valid());
}
