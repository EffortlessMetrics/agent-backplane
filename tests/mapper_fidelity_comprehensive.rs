#![allow(clippy::all)]
#![allow(unused_imports)]
#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_mut)]
#![allow(unreachable_code)]

//! Comprehensive mapper validation and fidelity tests.
//!
//! Verifies that cross-dialect mapping preserves semantic meaning and flags
//! lossy conversions correctly. Covers identity passthrough, tool mapping,
//! role mapping, streaming events, lossy detection, and error cases.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage};
use abp_core::{AgentEvent, AgentEventKind};
use abp_dialect::Dialect;
use abp_mapper::validation::{
    DefaultMappingValidator, MappingValidator, RoundtripResult, ValidationPipeline,
    ValidationSeverity,
};
use abp_mapper::{
    ClaudeGeminiIrMapper, ClaudeKimiIrMapper, ClaudeToOpenAiMapper, CodexClaudeIrMapper,
    DialectRequest, DialectResponse, GeminiKimiIrMapper, GeminiToOpenAiMapper, IdentityMapper,
    IrIdentityMapper, IrMapper, MapError, Mapper, MappingError, OpenAiClaudeIrMapper,
    OpenAiCodexIrMapper, OpenAiCopilotIrMapper, OpenAiGeminiIrMapper, OpenAiKimiIrMapper,
    OpenAiToClaudeMapper, OpenAiToGeminiMapper,
};
use chrono::Utc;
use serde_json::{Value, json};
use std::collections::BTreeMap;

// ═══════════════════════════════════════════════════════════════════════════
// § 1  LOSSLESS PASSTHROUGH VERIFICATION (20+ tests)
// ═══════════════════════════════════════════════════════════════════════════

mod lossless_passthrough {
    use super::*;

    // ── Identity mapper per dialect ─────────────────────────────────────

    #[test]
    fn identity_openai_request_bitwise() {
        let body = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "hello"}],
            "temperature": 0.7,
            "max_tokens": 1024
        });
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: body.clone(),
        };
        let result = IdentityMapper.map_request(&req).unwrap();
        assert_eq!(result, body);
    }

    #[test]
    fn identity_claude_request_bitwise() {
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "max_tokens": 4096,
            "messages": [{"role": "user", "content": "test"}],
            "system": "You are helpful"
        });
        let req = DialectRequest {
            dialect: Dialect::Claude,
            body: body.clone(),
        };
        let result = IdentityMapper.map_request(&req).unwrap();
        assert_eq!(result, body);
    }

    #[test]
    fn identity_gemini_request_bitwise() {
        let body = json!({
            "model": "gemini-pro",
            "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
            "generationConfig": {"temperature": 0.5}
        });
        let req = DialectRequest {
            dialect: Dialect::Gemini,
            body: body.clone(),
        };
        let result = IdentityMapper.map_request(&req).unwrap();
        assert_eq!(result, body);
    }

    #[test]
    fn identity_kimi_request_bitwise() {
        let body = json!({
            "model": "moonshot-v1-8k",
            "messages": [{"role": "user", "content": "test"}],
            "temperature": 0.3
        });
        let req = DialectRequest {
            dialect: Dialect::Kimi,
            body: body.clone(),
        };
        let result = IdentityMapper.map_request(&req).unwrap();
        assert_eq!(result, body);
    }

    #[test]
    fn identity_codex_request_bitwise() {
        let body = json!({
            "model": "codex-mini",
            "messages": [{"role": "user", "content": "write code"}]
        });
        let req = DialectRequest {
            dialect: Dialect::Codex,
            body: body.clone(),
        };
        let result = IdentityMapper.map_request(&req).unwrap();
        assert_eq!(result, body);
    }

    #[test]
    fn identity_copilot_request_bitwise() {
        let body = json!({
            "model": "copilot-gpt-4",
            "messages": [{"role": "system", "content": "help"}, {"role": "user", "content": "fix bug"}]
        });
        let req = DialectRequest {
            dialect: Dialect::Copilot,
            body: body.clone(),
        };
        let result = IdentityMapper.map_request(&req).unwrap();
        assert_eq!(result, body);
    }

    // ── IR identity mapper per dialect ──────────────────────────────────

    #[test]
    fn ir_identity_openai_roundtrip() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "You are helpful"),
            IrMessage::text(IrRole::User, "hi"),
            IrMessage::text(IrRole::Assistant, "hello"),
        ]);
        let result = IrIdentityMapper
            .map_request(Dialect::OpenAi, Dialect::OpenAi, &conv)
            .unwrap();
        assert_eq!(result, conv);
    }

    #[test]
    fn ir_identity_claude_roundtrip() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "test"),
            IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::Thinking { text: "hmm".into() }],
            ),
        ]);
        let result = IrIdentityMapper
            .map_request(Dialect::Claude, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(result, conv);
    }

    #[test]
    fn ir_identity_gemini_roundtrip() {
        let conv =
            IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "gemini test")]);
        let result = IrIdentityMapper
            .map_request(Dialect::Gemini, Dialect::Gemini, &conv)
            .unwrap();
        assert_eq!(result, conv);
    }

    #[test]
    fn ir_identity_kimi_roundtrip() {
        let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "kimi test")]);
        let result = IrIdentityMapper
            .map_request(Dialect::Kimi, Dialect::Kimi, &conv)
            .unwrap();
        assert_eq!(result, conv);
    }

    #[test]
    fn ir_identity_codex_roundtrip() {
        let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "codex test")]);
        let result = IrIdentityMapper
            .map_request(Dialect::Codex, Dialect::Codex, &conv)
            .unwrap();
        assert_eq!(result, conv);
    }

    #[test]
    fn ir_identity_copilot_roundtrip() {
        let conv =
            IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "copilot test")]);
        let result = IrIdentityMapper
            .map_request(Dialect::Copilot, Dialect::Copilot, &conv)
            .unwrap();
        assert_eq!(result, conv);
    }

    // ── Streaming events passthrough ────────────────────────────────────

    #[test]
    fn identity_event_assistant_delta_passthrough() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "streaming token".into(),
            },
            ext: None,
        };
        let result = IdentityMapper.map_event(&event).unwrap();
        assert_eq!(result["text"], "streaming token");
    }

    #[test]
    fn identity_event_tool_call_passthrough() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu_1".into()),
                parent_tool_use_id: None,
                input: json!({"path": "src/main.rs"}),
            },
            ext: None,
        };
        let result = IdentityMapper.map_event(&event).unwrap();
        assert_eq!(result["tool_name"], "read_file");
        assert_eq!(result["tool_use_id"], "tu_1");
    }

    #[test]
    fn identity_event_tool_result_passthrough() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu_1".into()),
                output: json!("file contents"),
                is_error: false,
            },
            ext: None,
        };
        let result = IdentityMapper.map_event(&event).unwrap();
        assert_eq!(result["tool_name"], "read_file");
        assert_eq!(result["is_error"], false);
    }

    // ── Response passthrough ────────────────────────────────────────────

    #[test]
    fn identity_response_passthrough_preserves_body() {
        let body = json!({
            "id": "chatcmpl-abc",
            "object": "chat.completion",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Hello!"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        });
        let resp = IdentityMapper.map_response(&body).unwrap();
        assert_eq!(resp.body, body);
    }

    #[test]
    fn identity_response_with_nested_tool_calls() {
        let body = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call_1",
                        "function": {"name": "bash", "arguments": "{\"cmd\":\"ls\"}"}
                    }]
                }
            }]
        });
        let resp = IdentityMapper.map_response(&body).unwrap();
        assert_eq!(resp.body, body);
    }

    #[test]
    fn ir_identity_response_preserves_tool_use_blocks() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "tu_1".into(),
                name: "bash".into(),
                input: json!({"cmd": "ls"}),
            }],
        )]);
        let result = IrIdentityMapper
            .map_response(Dialect::OpenAi, Dialect::OpenAi, &conv)
            .unwrap();
        assert_eq!(result, conv);
    }

    #[test]
    fn ir_identity_preserves_metadata() {
        let mut metadata = BTreeMap::new();
        metadata.insert("custom_key".to_string(), json!("custom_value"));
        let conv = IrConversation::from_messages(vec![IrMessage {
            role: IrRole::User,
            content: vec![IrContentBlock::Text {
                text: "test".into(),
            }],
            metadata,
        }]);
        let result = IrIdentityMapper
            .map_request(Dialect::OpenAi, Dialect::OpenAi, &conv)
            .unwrap();
        assert_eq!(
            result.messages[0].metadata["custom_key"],
            json!("custom_value")
        );
    }

    #[test]
    fn identity_all_dialects_supported() {
        let pairs = IrIdentityMapper.supported_pairs();
        for d in Dialect::all() {
            assert!(pairs.contains(&(*d, *d)));
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// § 2  CROSS-DIALECT TOOL MAPPING FIDELITY (25+ tests)
// ═══════════════════════════════════════════════════════════════════════════

mod cross_dialect_tool_mapping {
    use super::*;

    // ── OpenAI function_call → Claude tool_use ──────────────────────────

    #[test]
    fn openai_function_call_to_claude_tool_use_name_preserved() {
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "user", "content": "Weather?"},
                    {"role": "assistant", "content": null, "tool_calls": [{
                        "id": "call_abc",
                        "type": "function",
                        "function": {"name": "get_weather", "arguments": "{\"city\":\"NYC\"}"}
                    }]}
                ]
            }),
        };
        let result = OpenAiToClaudeMapper.map_request(&req).unwrap();
        let asst = &result["messages"][1]["content"];
        assert_eq!(asst[0]["type"], "tool_use");
        assert_eq!(asst[0]["name"], "get_weather");
        assert_eq!(asst[0]["id"], "call_abc");
        assert_eq!(asst[0]["input"]["city"], "NYC");
    }

    #[test]
    fn openai_tool_definition_to_claude_preserves_schema() {
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}],
                "tools": [{
                    "type": "function",
                    "function": {
                        "name": "search",
                        "description": "Search the web",
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "query": {"type": "string", "description": "Search query"},
                                "limit": {"type": "integer", "default": 10}
                            },
                            "required": ["query"]
                        }
                    }
                }]
            }),
        };
        let result = OpenAiToClaudeMapper.map_request(&req).unwrap();
        let tool = &result["tools"][0];
        assert_eq!(tool["name"], "search");
        assert_eq!(tool["description"], "Search the web");
        assert_eq!(
            tool["input_schema"]["properties"]["query"]["type"],
            "string"
        );
        assert_eq!(tool["input_schema"]["required"], json!(["query"]));
    }

    // ── Claude tool_result → OpenAI tool response ───────────────────────

    #[test]
    fn openai_tool_result_to_claude_content_preserved() {
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "user", "content": "Weather?"},
                    {"role": "tool", "tool_call_id": "call_1", "content": "72°F, sunny"}
                ]
            }),
        };
        let result = OpenAiToClaudeMapper.map_request(&req).unwrap();
        let tool_msg = &result["messages"][1];
        assert_eq!(tool_msg["role"], "user");
        assert_eq!(tool_msg["content"][0]["type"], "tool_result");
        assert_eq!(tool_msg["content"][0]["tool_use_id"], "call_1");
        assert_eq!(tool_msg["content"][0]["content"], "72°F, sunny");
    }

    // ── Gemini functionCall → OpenAI function_call ──────────────────────

    #[test]
    fn gemini_function_call_to_openai() {
        let req = DialectRequest {
            dialect: Dialect::Gemini,
            body: json!({
                "model": "gemini-pro",
                "contents": [
                    {"role": "user", "parts": [{"text": "Weather?"}]},
                    {"role": "model", "parts": [{
                        "functionCall": {"name": "get_weather", "args": {"city": "NYC"}}
                    }]}
                ]
            }),
        };
        let result = GeminiToOpenAiMapper.map_request(&req).unwrap();
        let asst = &result["messages"][1];
        assert_eq!(asst["role"], "assistant");
        assert!(asst["tool_calls"][0]["function"]["name"] == "get_weather");
    }

    #[test]
    fn gemini_function_response_to_openai_tool() {
        let req = DialectRequest {
            dialect: Dialect::Gemini,
            body: json!({
                "model": "gemini-pro",
                "contents": [{
                    "role": "user",
                    "parts": [{"functionResponse": {"name": "get_weather", "response": "72°F"}}]
                }]
            }),
        };
        let result = GeminiToOpenAiMapper.map_request(&req).unwrap();
        assert_eq!(result["messages"][0]["role"], "tool");
        assert_eq!(result["messages"][0]["name"], "get_weather");
    }

    // ── Tool definitions with complex JSON schemas ──────────────────────

    #[test]
    fn complex_schema_preserved_openai_to_claude() {
        let schema = json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "filters": {
                    "type": "object",
                    "properties": {
                        "date_range": {
                            "type": "object",
                            "properties": {
                                "start": {"type": "string", "format": "date"},
                                "end": {"type": "string", "format": "date"}
                            }
                        },
                        "tags": {"type": "array", "items": {"type": "string"}}
                    }
                }
            },
            "required": ["query"]
        });
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "search"}],
                "tools": [{"type": "function", "function": {
                    "name": "advanced_search",
                    "description": "Search with filters",
                    "parameters": schema.clone()
                }}]
            }),
        };
        let result = OpenAiToClaudeMapper.map_request(&req).unwrap();
        assert_eq!(result["tools"][0]["input_schema"], schema);
    }

    #[test]
    fn complex_schema_preserved_openai_to_gemini() {
        let schema = json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "options": {
                    "type": "object",
                    "properties": {
                        "recursive": {"type": "boolean"},
                        "depth": {"type": "integer", "minimum": 0}
                    }
                }
            }
        });
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "list"}],
                "tools": [{"type": "function", "function": {
                    "name": "list_dir",
                    "description": "List directory",
                    "parameters": schema.clone()
                }}]
            }),
        };
        let result = OpenAiToGeminiMapper.map_request(&req).unwrap();
        assert_eq!(
            result["tools"][0]["function_declarations"][0]["parameters"],
            schema
        );
    }

    // ── Tool definitions with optional parameters ───────────────────────

    #[test]
    fn optional_params_preserved_openai_to_claude() {
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}],
                "tools": [{"type": "function", "function": {
                    "name": "greet",
                    "description": "Greet someone",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string"},
                            "greeting": {"type": "string", "default": "Hello"}
                        },
                        "required": ["name"]
                    }
                }}]
            }),
        };
        let result = OpenAiToClaudeMapper.map_request(&req).unwrap();
        let schema = &result["tools"][0]["input_schema"];
        assert!(schema["properties"]["greeting"]["default"] == "Hello");
        assert_eq!(schema["required"], json!(["name"]));
    }

    // ── Tool definitions with nested object schemas ─────────────────────

    #[test]
    fn nested_object_schema_openai_to_claude() {
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}],
                "tools": [{"type": "function", "function": {
                    "name": "create_user",
                    "description": "Create a user",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "user": {
                                "type": "object",
                                "properties": {
                                    "name": {"type": "string"},
                                    "address": {
                                        "type": "object",
                                        "properties": {
                                            "street": {"type": "string"},
                                            "city": {"type": "string"},
                                            "zip": {"type": "string"}
                                        },
                                        "required": ["street", "city"]
                                    }
                                },
                                "required": ["name"]
                            }
                        },
                        "required": ["user"]
                    }
                }}]
            }),
        };
        let result = OpenAiToClaudeMapper.map_request(&req).unwrap();
        let addr =
            &result["tools"][0]["input_schema"]["properties"]["user"]["properties"]["address"];
        assert_eq!(addr["properties"]["street"]["type"], "string");
        assert_eq!(addr["required"], json!(["street", "city"]));
    }

    #[test]
    fn nested_object_schema_openai_to_gemini() {
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}],
                "tools": [{"type": "function", "function": {
                    "name": "create_user",
                    "description": "Create user",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "profile": {
                                "type": "object",
                                "properties": {
                                    "name": {"type": "string"},
                                    "age": {"type": "integer"}
                                }
                            }
                        }
                    }
                }}]
            }),
        };
        let result = OpenAiToGeminiMapper.map_request(&req).unwrap();
        let params = &result["tools"][0]["function_declarations"][0]["parameters"];
        assert_eq!(
            params["properties"]["profile"]["properties"]["name"]["type"],
            "string"
        );
    }

    // ── IR-level tool mapping ───────────────────────────────────────────

    #[test]
    fn ir_openai_to_claude_tool_use_preserved() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "Weather?"),
            IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::ToolUse {
                    id: "tu_1".into(),
                    name: "get_weather".into(),
                    input: json!({"city": "NYC"}),
                }],
            ),
        ]);
        let result = OpenAiClaudeIrMapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        let tool_block = &result.messages[1].content[0];
        match tool_block {
            IrContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "tu_1");
                assert_eq!(name, "get_weather");
                assert_eq!(input["city"], "NYC");
            }
            _ => panic!("expected ToolUse block"),
        }
    }

    #[test]
    fn ir_openai_to_claude_tool_result_role_change() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "tu_1".into(),
                content: vec![IrContentBlock::Text {
                    text: "72°F".into(),
                }],
                is_error: false,
            }],
        )]);
        let result = OpenAiClaudeIrMapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(result.messages[0].role, IrRole::User);
    }

    #[test]
    fn ir_claude_to_openai_tool_result_role_restored() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "tu_1".into(),
                content: vec![IrContentBlock::Text {
                    text: "result".into(),
                }],
                is_error: false,
            }],
        )]);
        let result = OpenAiClaudeIrMapper
            .map_request(Dialect::Claude, Dialect::OpenAi, &conv)
            .unwrap();
        assert_eq!(result.messages[0].role, IrRole::Tool);
    }

    #[test]
    fn ir_openai_to_gemini_tool_role_becomes_user() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "tu_1".into(),
                content: vec![IrContentBlock::Text { text: "ok".into() }],
                is_error: false,
            }],
        )]);
        let result = OpenAiGeminiIrMapper
            .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
            .unwrap();
        assert_eq!(result.messages[0].role, IrRole::User);
    }

    #[test]
    fn ir_claude_to_gemini_tool_use_preserved() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "tu_abc".into(),
                name: "search".into(),
                input: json!({"q": "rust"}),
            }],
        )]);
        let result = ClaudeGeminiIrMapper
            .map_request(Dialect::Claude, Dialect::Gemini, &conv)
            .unwrap();
        match &result.messages[0].content[0] {
            IrContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "tu_abc");
                assert_eq!(name, "search");
                assert_eq!(input["q"], "rust");
            }
            _ => panic!("expected ToolUse"),
        }
    }

    #[test]
    fn ir_gemini_to_kimi_tool_result_role_change() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "tu_1".into(),
                content: vec![IrContentBlock::Text {
                    text: "result".into(),
                }],
                is_error: false,
            }],
        )]);
        let result = GeminiKimiIrMapper
            .map_request(Dialect::Gemini, Dialect::Kimi, &conv)
            .unwrap();
        assert_eq!(result.messages[0].role, IrRole::Tool);
    }

    #[test]
    fn ir_kimi_to_claude_tool_role_to_user() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "tu_1".into(),
                content: vec![IrContentBlock::Text { text: "ok".into() }],
                is_error: false,
            }],
        )]);
        let result = ClaudeKimiIrMapper
            .map_request(Dialect::Kimi, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(result.messages[0].role, IrRole::User);
    }

    #[test]
    fn multiple_tools_openai_to_claude() {
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}],
                "tools": [
                    {"type": "function", "function": {"name": "tool_a", "description": "A", "parameters": {"type": "object", "properties": {}}}},
                    {"type": "function", "function": {"name": "tool_b", "description": "B", "parameters": {"type": "object", "properties": {}}}},
                    {"type": "function", "function": {"name": "tool_c", "description": "C", "parameters": {"type": "object", "properties": {}}}}
                ]
            }),
        };
        let result = OpenAiToClaudeMapper.map_request(&req).unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 3);
        assert_eq!(tools[0]["name"], "tool_a");
        assert_eq!(tools[1]["name"], "tool_b");
        assert_eq!(tools[2]["name"], "tool_c");
    }

    #[test]
    fn multiple_tools_openai_to_gemini() {
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}],
                "tools": [
                    {"type": "function", "function": {"name": "search", "description": "Search", "parameters": {"type": "object"}}},
                    {"type": "function", "function": {"name": "read", "description": "Read", "parameters": {"type": "object"}}}
                ]
            }),
        };
        let result = OpenAiToGeminiMapper.map_request(&req).unwrap();
        let decls = result["tools"][0]["function_declarations"]
            .as_array()
            .unwrap();
        assert_eq!(decls.len(), 2);
        assert_eq!(decls[0]["name"], "search");
        assert_eq!(decls[1]["name"], "read");
    }

    #[test]
    fn gemini_function_declarations_roundtrip() {
        let openai_body = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [{"type": "function", "function": {
                "name": "calc",
                "description": "Calculate",
                "parameters": {"type": "object", "properties": {"expr": {"type": "string"}}}
            }}]
        });
        let gemini = OpenAiToGeminiMapper
            .map_request(&DialectRequest {
                dialect: Dialect::OpenAi,
                body: openai_body,
            })
            .unwrap();
        let back = GeminiToOpenAiMapper
            .map_request(&DialectRequest {
                dialect: Dialect::Gemini,
                body: gemini,
            })
            .unwrap();
        assert_eq!(back["tools"][0]["function"]["name"], "calc");
        assert_eq!(
            back["tools"][0]["function"]["parameters"]["properties"]["expr"]["type"],
            "string"
        );
    }

    #[test]
    fn tool_with_array_items_schema() {
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}],
                "tools": [{"type": "function", "function": {
                    "name": "batch",
                    "description": "Batch operation",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "items": {
                                "type": "array",
                                "items": {"type": "string"},
                                "minItems": 1,
                                "maxItems": 100
                            }
                        }
                    }
                }}]
            }),
        };
        let result = OpenAiToClaudeMapper.map_request(&req).unwrap();
        let items_schema = &result["tools"][0]["input_schema"]["properties"]["items"];
        assert_eq!(items_schema["type"], "array");
        assert_eq!(items_schema["items"]["type"], "string");
    }

    #[test]
    fn tool_with_enum_schema() {
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}],
                "tools": [{"type": "function", "function": {
                    "name": "set_mode",
                    "description": "Set mode",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "mode": {"type": "string", "enum": ["fast", "slow", "balanced"]}
                        }
                    }
                }}]
            }),
        };
        let result = OpenAiToClaudeMapper.map_request(&req).unwrap();
        let mode = &result["tools"][0]["input_schema"]["properties"]["mode"];
        assert_eq!(mode["enum"], json!(["fast", "slow", "balanced"]));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// § 3  MESSAGE ROLE MAPPING (20+ tests)
// ═══════════════════════════════════════════════════════════════════════════

mod message_role_mapping {
    use super::*;

    // ── OpenAI roles → Claude ───────────────────────────────────────────

    #[test]
    fn openai_system_role_extracted_to_claude_top_level() {
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "system", "content": "You are a helpful assistant."},
                    {"role": "user", "content": "Hi"}
                ]
            }),
        };
        let result = OpenAiToClaudeMapper.map_request(&req).unwrap();
        assert_eq!(result["system"], "You are a helpful assistant.");
        assert_eq!(result["messages"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn openai_user_role_preserved_in_claude() {
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "test"}]
            }),
        };
        let result = OpenAiToClaudeMapper.map_request(&req).unwrap();
        assert_eq!(result["messages"][0]["role"], "user");
    }

    #[test]
    fn openai_assistant_role_preserved_in_claude() {
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "user", "content": "Hi"},
                    {"role": "assistant", "content": "Hello!"}
                ]
            }),
        };
        let result = OpenAiToClaudeMapper.map_request(&req).unwrap();
        assert_eq!(result["messages"][1]["role"], "assistant");
    }

    #[test]
    fn openai_tool_role_to_claude_user_with_tool_result() {
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "user", "content": "Weather?"},
                    {"role": "tool", "tool_call_id": "call_1", "content": "sunny"}
                ]
            }),
        };
        let result = OpenAiToClaudeMapper.map_request(&req).unwrap();
        assert_eq!(result["messages"][1]["role"], "user");
        assert_eq!(result["messages"][1]["content"][0]["type"], "tool_result");
    }

    // ── OpenAI roles → Gemini ───────────────────────────────────────────

    #[test]
    fn openai_system_to_gemini_system_instruction() {
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
        let result = OpenAiToGeminiMapper.map_request(&req).unwrap();
        assert_eq!(
            result["system_instruction"]["parts"][0]["text"],
            "Be concise."
        );
    }

    #[test]
    fn openai_assistant_to_gemini_model_role() {
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "user", "content": "Hi"},
                    {"role": "assistant", "content": "Hello!"}
                ]
            }),
        };
        let result = OpenAiToGeminiMapper.map_request(&req).unwrap();
        assert_eq!(result["contents"][1]["role"], "model");
    }

    #[test]
    fn openai_user_to_gemini_user_with_parts() {
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hello"}]
            }),
        };
        let result = OpenAiToGeminiMapper.map_request(&req).unwrap();
        assert_eq!(result["contents"][0]["role"], "user");
        assert_eq!(result["contents"][0]["parts"][0]["text"], "hello");
    }

    // ── Gemini roles → OpenAI ───────────────────────────────────────────

    #[test]
    fn gemini_system_instruction_to_openai_system_role() {
        let req = DialectRequest {
            dialect: Dialect::Gemini,
            body: json!({
                "model": "gemini-pro",
                "system_instruction": {"parts": [{"text": "Be helpful"}]},
                "contents": [{"role": "user", "parts": [{"text": "Hi"}]}]
            }),
        };
        let result = GeminiToOpenAiMapper.map_request(&req).unwrap();
        assert_eq!(result["messages"][0]["role"], "system");
        assert_eq!(result["messages"][0]["content"], "Be helpful");
    }

    #[test]
    fn gemini_model_to_openai_assistant() {
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
        let result = GeminiToOpenAiMapper.map_request(&req).unwrap();
        assert_eq!(result["messages"][1]["role"], "assistant");
    }

    // ── Multi-turn conversation order ───────────────────────────────────

    #[test]
    fn multi_turn_order_preserved_openai_to_claude() {
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "system", "content": "You are helpful"},
                    {"role": "user", "content": "First"},
                    {"role": "assistant", "content": "Response 1"},
                    {"role": "user", "content": "Second"},
                    {"role": "assistant", "content": "Response 2"}
                ]
            }),
        };
        let result = OpenAiToClaudeMapper.map_request(&req).unwrap();
        let msgs = result["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 4); // system extracted
        assert_eq!(msgs[0]["content"], "First");
        assert_eq!(msgs[1]["content"][0]["text"], "Response 1");
        assert_eq!(msgs[2]["content"], "Second");
        assert_eq!(msgs[3]["content"][0]["text"], "Response 2");
    }

    #[test]
    fn multi_turn_order_preserved_openai_to_gemini() {
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "user", "content": "A"},
                    {"role": "assistant", "content": "B"},
                    {"role": "user", "content": "C"}
                ]
            }),
        };
        let result = OpenAiToGeminiMapper.map_request(&req).unwrap();
        let contents = result["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 3);
        assert_eq!(contents[0]["parts"][0]["text"], "A");
        assert_eq!(contents[1]["parts"][0]["text"], "B");
        assert_eq!(contents[2]["parts"][0]["text"], "C");
    }

    // ── Empty and long messages ─────────────────────────────────────────

    #[test]
    fn empty_content_message_openai_to_claude() {
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "user", "content": ""},
                    {"role": "assistant", "content": ""}
                ]
            }),
        };
        let result = OpenAiToClaudeMapper.map_request(&req).unwrap();
        assert!(result["messages"].as_array().is_some());
    }

    #[test]
    fn very_long_message_openai_to_claude() {
        let long_text = "x".repeat(100_000);
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": long_text}]
            }),
        };
        let result = OpenAiToClaudeMapper.map_request(&req).unwrap();
        assert_eq!(
            result["messages"][0]["content"].as_str().unwrap().len(),
            100_000
        );
    }

    #[test]
    fn very_long_message_openai_to_gemini() {
        let long_text = "y".repeat(50_000);
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": long_text}]
            }),
        };
        let result = OpenAiToGeminiMapper.map_request(&req).unwrap();
        assert_eq!(
            result["contents"][0]["parts"][0]["text"]
                .as_str()
                .unwrap()
                .len(),
            50_000
        );
    }

    // ── Messages with mixed content types ───────────────────────────────

    #[test]
    fn mixed_text_image_openai_to_gemini() {
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
        let result = OpenAiToGeminiMapper.map_request(&req).unwrap();
        let parts = result["contents"][0]["parts"].as_array().unwrap();
        assert_eq!(parts[0]["text"], "What is this?");
        assert!(parts[1].get("inlineData").is_some());
    }

    #[test]
    fn ir_system_message_preserved_openai_claude() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "You are helpful"),
            IrMessage::text(IrRole::User, "Hi"),
        ]);
        let result = OpenAiClaudeIrMapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(result.messages[0].role, IrRole::System);
        assert_eq!(result.messages[0].text_content(), "You are helpful");
    }

    #[test]
    fn ir_multi_turn_order_preserved() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "1"),
            IrMessage::text(IrRole::Assistant, "2"),
            IrMessage::text(IrRole::User, "3"),
            IrMessage::text(IrRole::Assistant, "4"),
        ]);
        let result = OpenAiClaudeIrMapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(result.messages.len(), 4);
        for (i, msg) in result.messages.iter().enumerate() {
            assert_eq!(msg.text_content(), (i + 1).to_string());
        }
    }

    #[test]
    fn ir_user_with_mixed_content_openai_to_claude() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::User,
            vec![
                IrContentBlock::Text {
                    text: "Look at this".into(),
                },
                IrContentBlock::Image {
                    media_type: "image/png".into(),
                    data: "base64data".into(),
                },
            ],
        )]);
        let result = OpenAiClaudeIrMapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(result.messages[0].content.len(), 2);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// § 4  STREAMING EVENT FIDELITY (20+ tests)
// ═══════════════════════════════════════════════════════════════════════════

mod streaming_event_fidelity {
    use super::*;

    // ── SSE delta → Claude streaming events ─────────────────────────────

    #[test]
    fn openai_delta_to_claude_content_block_delta() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "Hello".into(),
            },
            ext: None,
        };
        let result = OpenAiToClaudeMapper.map_event(&event).unwrap();
        assert_eq!(result["type"], "content_block_delta");
        assert_eq!(result["delta"]["type"], "text_delta");
        assert_eq!(result["delta"]["text"], "Hello");
    }

    #[test]
    fn openai_message_to_claude_message_event() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Complete response".into(),
            },
            ext: None,
        };
        let result = OpenAiToClaudeMapper.map_event(&event).unwrap();
        assert_eq!(result["type"], "message");
        assert_eq!(result["role"], "assistant");
        assert_eq!(result["content"][0]["text"], "Complete response");
    }

    // ── Tool call streaming ─────────────────────────────────────────────

    #[test]
    fn tool_call_event_to_claude_content_block_start() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "bash".into(),
                tool_use_id: Some("tu_42".into()),
                parent_tool_use_id: None,
                input: json!({"command": "ls -la"}),
            },
            ext: None,
        };
        let result = OpenAiToClaudeMapper.map_event(&event).unwrap();
        assert_eq!(result["type"], "content_block_start");
        assert_eq!(result["content_block"]["type"], "tool_use");
        assert_eq!(result["content_block"]["name"], "bash");
        assert_eq!(result["content_block"]["id"], "tu_42");
    }

    #[test]
    fn tool_call_event_to_gemini_function_call() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "search".into(),
                tool_use_id: Some("c1".into()),
                parent_tool_use_id: None,
                input: json!({"q": "rust async"}),
            },
            ext: None,
        };
        let result = OpenAiToGeminiMapper.map_event(&event).unwrap();
        let fc = &result["candidates"][0]["content"]["parts"][0]["functionCall"];
        assert_eq!(fc["name"], "search");
        assert_eq!(fc["args"]["q"], "rust async");
    }

    #[test]
    fn tool_result_event_to_claude_format() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: "bash".into(),
                tool_use_id: Some("tu_42".into()),
                output: json!("file1.txt\nfile2.txt"),
                is_error: false,
            },
            ext: None,
        };
        let result = OpenAiToClaudeMapper.map_event(&event).unwrap();
        assert_eq!(result["type"], "tool_result");
        assert_eq!(result["tool_use_id"], "tu_42");
        assert_eq!(result["is_error"], false);
    }

    #[test]
    fn tool_result_event_to_gemini_function_response() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: "search".into(),
                tool_use_id: Some("c1".into()),
                output: json!({"results": ["a", "b"]}),
                is_error: false,
            },
            ext: None,
        };
        let result = OpenAiToGeminiMapper.map_event(&event).unwrap();
        let fr = &result["candidates"][0]["content"]["parts"][0]["functionResponse"];
        assert_eq!(fr["name"], "search");
    }

    // ── Gemini → OpenAI streaming ───────────────────────────────────────

    #[test]
    fn gemini_delta_to_openai_chunk() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "streaming".into(),
            },
            ext: None,
        };
        let result = GeminiToOpenAiMapper.map_event(&event).unwrap();
        assert_eq!(result["object"], "chat.completion.chunk");
        assert_eq!(result["choices"][0]["delta"]["content"], "streaming");
    }

    #[test]
    fn gemini_message_to_openai_completion() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "done".into(),
            },
            ext: None,
        };
        let result = GeminiToOpenAiMapper.map_event(&event).unwrap();
        assert_eq!(result["object"], "chat.completion");
        assert_eq!(result["choices"][0]["finish_reason"], "stop");
    }

    #[test]
    fn gemini_tool_call_to_openai_tool_call_chunk() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "search".into(),
                tool_use_id: Some("c1".into()),
                parent_tool_use_id: None,
                input: json!({"q": "test"}),
            },
            ext: None,
        };
        let result = GeminiToOpenAiMapper.map_event(&event).unwrap();
        let tc = &result["choices"][0]["delta"]["tool_calls"][0];
        assert_eq!(tc["id"], "c1");
        assert_eq!(tc["function"]["name"], "search");
    }

    #[test]
    fn gemini_tool_result_to_openai_tool_message() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: "search".into(),
                tool_use_id: Some("c1".into()),
                output: json!("result text"),
                is_error: false,
            },
            ext: None,
        };
        let result = GeminiToOpenAiMapper.map_event(&event).unwrap();
        assert_eq!(result["role"], "tool");
        assert_eq!(result["tool_call_id"], "c1");
    }

    // ── Stop reasons ────────────────────────────────────────────────────

    #[test]
    fn openai_stop_reason_in_gemini_event() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "done".into(),
            },
            ext: None,
        };
        let result = OpenAiToGeminiMapper.map_event(&event).unwrap();
        assert_eq!(result["candidates"][0]["finishReason"], "STOP");
    }

    #[test]
    fn gemini_stop_to_openai_finish_reason() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "final".into(),
            },
            ext: None,
        };
        let result = GeminiToOpenAiMapper.map_event(&event).unwrap();
        assert_eq!(result["choices"][0]["finish_reason"], "stop");
    }

    // ── Warning/Error events ────────────────────────────────────────────

    #[test]
    fn warning_event_maps_through_claude() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Warning {
                message: "rate limit".into(),
            },
            ext: None,
        };
        let result = OpenAiToClaudeMapper.map_event(&event).unwrap();
        assert_eq!(result["type"], "warning");
    }

    #[test]
    fn warning_event_maps_through_gemini() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Warning {
                message: "approaching limit".into(),
            },
            ext: None,
        };
        let result = OpenAiToGeminiMapper.map_event(&event).unwrap();
        assert_eq!(result["type"], "warning");
    }

    #[test]
    fn error_event_maps_through_openai_to_claude() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error {
                message: "timeout".into(),
                error_code: None,
            },
            ext: None,
        };
        let result = OpenAiToClaudeMapper.map_event(&event).unwrap();
        assert_eq!(result["type"], "error");
    }

    #[test]
    fn run_started_event_generic_passthrough() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "starting".into(),
            },
            ext: None,
        };
        let result = OpenAiToClaudeMapper.map_event(&event).unwrap();
        assert_eq!(result["type"], "run_started");
    }

    #[test]
    fn run_completed_event_generic_passthrough() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        };
        let result = OpenAiToGeminiMapper.map_event(&event).unwrap();
        assert_eq!(result["type"], "run_completed");
    }

    #[test]
    fn file_changed_event_passthrough() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::FileChanged {
                path: "src/main.rs".into(),
                summary: "added function".into(),
            },
            ext: None,
        };
        let result = IdentityMapper.map_event(&event).unwrap();
        assert_eq!(result["type"], "file_changed");
        assert_eq!(result["path"], "src/main.rs");
    }

    #[test]
    fn command_executed_event_passthrough() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::CommandExecuted {
                command: "cargo test".into(),
                exit_code: Some(0),
                output_preview: Some("all tests passed".into()),
            },
            ext: None,
        };
        let result = IdentityMapper.map_event(&event).unwrap();
        assert_eq!(result["type"], "command_executed");
        assert_eq!(result["command"], "cargo test");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// § 5  LOSSY MAPPING DETECTION (20+ tests)
// ═══════════════════════════════════════════════════════════════════════════

mod lossy_mapping_detection {
    use super::*;

    // ── OpenAI-only features → Claude ───────────────────────────────────

    #[test]
    fn openai_logprobs_not_in_claude_mapping() {
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}],
                "logprobs": true,
                "top_logprobs": 5
            }),
        };
        let result = OpenAiToClaudeMapper.map_request(&req).unwrap();
        assert!(result.get("logprobs").is_none());
        assert!(result.get("top_logprobs").is_none());
    }

    #[test]
    fn openai_logprobs_roundtrip_lossy() {
        let validator = DefaultMappingValidator::new();
        let original = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hi"}],
            "logprobs": true
        });
        let mapped = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 4096
        });
        let rt = validator.validate_roundtrip(&original, &mapped);
        assert!(!rt.is_lossless());
        assert!(rt.lost_fields.iter().any(|f| f.contains("logprobs")));
    }

    #[test]
    fn openai_response_format_not_in_claude() {
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}],
                "response_format": {"type": "json_object"}
            }),
        };
        let result = OpenAiToClaudeMapper.map_request(&req).unwrap();
        assert!(result.get("response_format").is_none());
    }

    // ── Claude-only features → OpenAI ───────────────────────────────────

    #[test]
    fn claude_thinking_blocks_dropped_in_openai() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "Think hard"),
            IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Thinking {
                        text: "Let me think...".into(),
                    },
                    IrContentBlock::Text {
                        text: "Here's the answer".into(),
                    },
                ],
            ),
        ]);
        let result = OpenAiClaudeIrMapper
            .map_request(Dialect::Claude, Dialect::OpenAi, &conv)
            .unwrap();
        let asst_content = &result.messages[1].content;
        assert_eq!(asst_content.len(), 1);
        assert!(matches!(asst_content[0], IrContentBlock::Text { .. }));
    }

    #[test]
    fn claude_thinking_blocks_preserved_claude_to_claude() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "thinking...".into(),
                },
                IrContentBlock::Text {
                    text: "answer".into(),
                },
            ],
        )]);
        let result = IrIdentityMapper
            .map_request(Dialect::Claude, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(result.messages[0].content.len(), 2);
        assert!(matches!(
            result.messages[0].content[0],
            IrContentBlock::Thinking { .. }
        ));
    }

    #[test]
    fn claude_thinking_blocks_dropped_in_gemini() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking { text: "hmm".into() },
                IrContentBlock::Text {
                    text: "result".into(),
                },
            ],
        )]);
        let result = ClaudeGeminiIrMapper
            .map_request(Dialect::Claude, Dialect::Gemini, &conv)
            .unwrap();
        assert_eq!(result.messages[0].content.len(), 1);
        assert!(matches!(
            result.messages[0].content[0],
            IrContentBlock::Text { .. }
        ));
    }

    #[test]
    fn claude_thinking_blocks_dropped_in_kimi() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "thinking".into(),
                },
                IrContentBlock::Text {
                    text: "answer".into(),
                },
            ],
        )]);
        let result = ClaudeKimiIrMapper
            .map_request(Dialect::Claude, Dialect::Kimi, &conv)
            .unwrap();
        assert_eq!(result.messages[0].content.len(), 1);
    }

    // ── Gemini-only features → others ───────────────────────────────────

    #[test]
    fn gemini_safety_settings_not_in_openai() {
        let req = DialectRequest {
            dialect: Dialect::Gemini,
            body: json!({
                "model": "gemini-pro",
                "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
                "safetySettings": [
                    {"category": "HARM_CATEGORY_HARASSMENT", "threshold": "BLOCK_MEDIUM_AND_ABOVE"}
                ]
            }),
        };
        let result = GeminiToOpenAiMapper.map_request(&req).unwrap();
        assert!(result.get("safetySettings").is_none());
        assert!(result.get("safety_settings").is_none());
    }

    #[test]
    fn gemini_grounding_config_not_in_openai() {
        let req = DialectRequest {
            dialect: Dialect::Gemini,
            body: json!({
                "model": "gemini-pro",
                "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
                "groundingConfig": {"sources": [{"type": "web"}]}
            }),
        };
        let result = GeminiToOpenAiMapper.map_request(&req).unwrap();
        assert!(result.get("groundingConfig").is_none());
    }

    // ── Codex output-only limitations ───────────────────────────────────

    #[test]
    fn codex_drops_system_messages() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "You are helpful"),
            IrMessage::text(IrRole::User, "Hello"),
        ]);
        let result = OpenAiCodexIrMapper
            .map_request(Dialect::OpenAi, Dialect::Codex, &conv)
            .unwrap();
        assert!(result.messages.iter().all(|m| m.role != IrRole::System));
    }

    #[test]
    fn codex_drops_tool_calls() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Let me check".into(),
                },
                IrContentBlock::ToolUse {
                    id: "tu_1".into(),
                    name: "bash".into(),
                    input: json!({"cmd": "ls"}),
                },
            ],
        )]);
        let result = OpenAiCodexIrMapper
            .map_request(Dialect::OpenAi, Dialect::Codex, &conv)
            .unwrap();
        assert_eq!(result.messages[0].content.len(), 1);
        assert!(matches!(
            result.messages[0].content[0],
            IrContentBlock::Text { .. }
        ));
    }

    #[test]
    fn codex_drops_tool_role_messages() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "Hello"),
            IrMessage::new(
                IrRole::Tool,
                vec![IrContentBlock::ToolResult {
                    tool_use_id: "tu_1".into(),
                    content: vec![IrContentBlock::Text {
                        text: "result".into(),
                    }],
                    is_error: false,
                }],
            ),
        ]);
        let result = OpenAiCodexIrMapper
            .map_request(Dialect::OpenAi, Dialect::Codex, &conv)
            .unwrap();
        assert_eq!(result.messages.len(), 1);
        assert_eq!(result.messages[0].role, IrRole::User);
    }

    #[test]
    fn codex_drops_thinking_blocks() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "thinking".into(),
                },
                IrContentBlock::Text {
                    text: "answer".into(),
                },
            ],
        )]);
        let result = OpenAiCodexIrMapper
            .map_request(Dialect::OpenAi, Dialect::Codex, &conv)
            .unwrap();
        assert_eq!(result.messages[0].content.len(), 1);
    }

    #[test]
    fn codex_drops_image_blocks() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::User,
            vec![
                IrContentBlock::Text {
                    text: "look".into(),
                },
                IrContentBlock::Image {
                    media_type: "image/png".into(),
                    data: "base64".into(),
                },
            ],
        )]);
        let result = OpenAiCodexIrMapper
            .map_request(Dialect::OpenAi, Dialect::Codex, &conv)
            .unwrap();
        // Codex emulates images as text placeholders, so both blocks survive.
        assert_eq!(result.messages[0].content.len(), 2);
        assert!(matches!(
            &result.messages[0].content[1],
            IrContentBlock::Text { text } if text.contains("[Image:")
        ));
    }

    #[test]
    fn codex_to_openai_is_lossless() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "Hello"),
            IrMessage::text(IrRole::Assistant, "World"),
        ]);
        let result = OpenAiCodexIrMapper
            .map_request(Dialect::Codex, Dialect::OpenAi, &conv)
            .unwrap();
        assert_eq!(result, conv);
    }

    // ── Temperature/top_p/max_tokens mapping ────────────────────────────

    #[test]
    fn temperature_mapped_openai_to_claude() {
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}],
                "temperature": 0.5
            }),
        };
        let result = OpenAiToClaudeMapper.map_request(&req).unwrap();
        assert_eq!(result["temperature"], 0.5);
    }

    #[test]
    fn temperature_mapped_openai_to_gemini_in_config() {
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}],
                "temperature": 0.8
            }),
        };
        let result = OpenAiToGeminiMapper.map_request(&req).unwrap();
        assert_eq!(result["generationConfig"]["temperature"], 0.8);
    }

    #[test]
    fn max_tokens_mapped_openai_to_gemini_config() {
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}],
                "max_tokens": 2048
            }),
        };
        let result = OpenAiToGeminiMapper.map_request(&req).unwrap();
        assert_eq!(result["generationConfig"]["maxOutputTokens"], 2048);
    }

    #[test]
    fn top_p_mapped_openai_to_gemini_config() {
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}],
                "top_p": 0.95
            }),
        };
        let result = OpenAiToGeminiMapper.map_request(&req).unwrap();
        assert_eq!(result["generationConfig"]["topP"], 0.95);
    }

    #[test]
    fn copilot_thinking_blocks_dropped() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking { text: "hmm".into() },
                IrContentBlock::Text {
                    text: "result".into(),
                },
            ],
        )]);
        let result = OpenAiCopilotIrMapper
            .map_request(Dialect::OpenAi, Dialect::Copilot, &conv)
            .unwrap();
        assert_eq!(result.messages[0].content.len(), 1);
        assert!(matches!(
            result.messages[0].content[0],
            IrContentBlock::Text { .. }
        ));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// § 6  ERROR CASES AND VALIDATION (25+ tests)
// ═══════════════════════════════════════════════════════════════════════════

mod error_cases_and_validation {
    use super::*;

    // ── Invalid tool schema ─────────────────────────────────────────────

    #[test]
    fn openai_tool_missing_function_field_errors() {
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}],
                "tools": [{"type": "function"}]
            }),
        };
        let err = OpenAiToClaudeMapper.map_request(&req).unwrap_err();
        assert!(matches!(err, MappingError::IncompatibleTypes { .. }));
    }

    #[test]
    fn gemini_tool_missing_function_field_errors_to_openai() {
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}],
                "tools": [{"type": "not_function"}]
            }),
        };
        let err = OpenAiToGeminiMapper.map_request(&req).unwrap_err();
        assert!(matches!(err, MappingError::IncompatibleTypes { .. }));
    }

    // ── Wrong dialect errors ────────────────────────────────────────────

    #[test]
    fn openai_to_claude_rejects_claude_dialect() {
        let req = DialectRequest {
            dialect: Dialect::Claude,
            body: json!({"model": "claude-3"}),
        };
        let err = OpenAiToClaudeMapper.map_request(&req).unwrap_err();
        assert!(matches!(err, MappingError::UnmappableRequest { .. }));
    }

    #[test]
    fn openai_to_gemini_rejects_gemini_dialect() {
        let req = DialectRequest {
            dialect: Dialect::Gemini,
            body: json!({"model": "gemini-pro"}),
        };
        let err = OpenAiToGeminiMapper.map_request(&req).unwrap_err();
        assert!(matches!(err, MappingError::UnmappableRequest { .. }));
    }

    #[test]
    fn gemini_to_openai_rejects_openai_dialect() {
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({"model": "gpt-4"}),
        };
        let err = GeminiToOpenAiMapper.map_request(&req).unwrap_err();
        assert!(matches!(err, MappingError::UnmappableRequest { .. }));
    }

    // ── Non-object body ─────────────────────────────────────────────────

    #[test]
    fn non_object_body_openai_to_claude() {
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!("a string"),
        };
        let err = OpenAiToClaudeMapper.map_request(&req).unwrap_err();
        assert!(matches!(err, MappingError::UnmappableRequest { .. }));
    }

    #[test]
    fn non_object_body_openai_to_gemini() {
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!(42),
        };
        let err = OpenAiToGeminiMapper.map_request(&req).unwrap_err();
        assert!(matches!(err, MappingError::UnmappableRequest { .. }));
    }

    #[test]
    fn non_object_body_gemini_to_openai() {
        let req = DialectRequest {
            dialect: Dialect::Gemini,
            body: json!([1, 2, 3]),
        };
        let err = GeminiToOpenAiMapper.map_request(&req).unwrap_err();
        assert!(matches!(err, MappingError::UnmappableRequest { .. }));
    }

    // ── Unknown role errors ─────────────────────────────────────────────

    #[test]
    fn unknown_role_openai_to_claude() {
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "unknown_role", "content": "hi"}]
            }),
        };
        let err = OpenAiToClaudeMapper.map_request(&req).unwrap_err();
        assert!(matches!(err, MappingError::IncompatibleTypes { .. }));
    }

    #[test]
    fn unknown_role_openai_to_gemini() {
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "custom_role", "content": "hi"}]
            }),
        };
        let err = OpenAiToGeminiMapper.map_request(&req).unwrap_err();
        assert!(matches!(err, MappingError::IncompatibleTypes { .. }));
    }

    #[test]
    fn unknown_role_gemini_to_openai() {
        let req = DialectRequest {
            dialect: Dialect::Gemini,
            body: json!({
                "model": "gemini-pro",
                "contents": [{"role": "unknown_role", "parts": [{"text": "hi"}]}]
            }),
        };
        let err = GeminiToOpenAiMapper.map_request(&req).unwrap_err();
        assert!(matches!(err, MappingError::IncompatibleTypes { .. }));
    }

    // ── IR unsupported pair errors ──────────────────────────────────────

    #[test]
    fn ir_unsupported_pair_openai_claude_mapper() {
        let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "hi")]);
        let err = OpenAiClaudeIrMapper
            .map_request(Dialect::Gemini, Dialect::Kimi, &conv)
            .unwrap_err();
        assert!(matches!(err, MapError::UnsupportedPair { .. }));
    }

    #[test]
    fn ir_unsupported_pair_openai_gemini_mapper() {
        let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "hi")]);
        let err = OpenAiGeminiIrMapper
            .map_request(Dialect::Claude, Dialect::Kimi, &conv)
            .unwrap_err();
        assert!(matches!(err, MapError::UnsupportedPair { .. }));
    }

    #[test]
    fn ir_unsupported_pair_claude_gemini_mapper() {
        let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "hi")]);
        let err = ClaudeGeminiIrMapper
            .map_request(Dialect::OpenAi, Dialect::Kimi, &conv)
            .unwrap_err();
        assert!(matches!(err, MapError::UnsupportedPair { .. }));
    }

    #[test]
    fn ir_unsupported_pair_openai_codex_mapper() {
        let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "hi")]);
        let err = OpenAiCodexIrMapper
            .map_request(Dialect::Claude, Dialect::Gemini, &conv)
            .unwrap_err();
        assert!(matches!(err, MapError::UnsupportedPair { .. }));
    }

    // ── Codex unmappable tools ──────────────────────────────────────────

    #[test]
    fn codex_apply_patch_unmappable_to_claude() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "tu_1".into(),
                name: "apply_patch".into(),
                input: json!({"patch": "diff"}),
            }],
        )]);
        let err = CodexClaudeIrMapper
            .map_request(Dialect::Codex, Dialect::Claude, &conv)
            .unwrap_err();
        assert!(matches!(err, MapError::UnmappableTool { .. }));
    }

    #[test]
    fn codex_apply_diff_unmappable_to_claude() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "tu_1".into(),
                name: "apply_diff".into(),
                input: json!({"diff": "contents"}),
            }],
        )]);
        let err = CodexClaudeIrMapper
            .map_request(Dialect::Codex, Dialect::Claude, &conv)
            .unwrap_err();
        assert!(matches!(err, MapError::UnmappableTool { .. }));
    }

    // ── Gemini system image blocks error ────────────────────────────────

    #[test]
    fn gemini_system_with_image_blocks_errors() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::System,
            vec![IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "base64".into(),
            }],
        )]);
        let err = ClaudeGeminiIrMapper
            .map_request(Dialect::Claude, Dialect::Gemini, &conv)
            .unwrap_err();
        assert!(matches!(err, MapError::UnmappableContent { .. }));
    }

    // ── Validation pipeline ─────────────────────────────────────────────

    #[test]
    fn validation_missing_model_openai() {
        let v = DefaultMappingValidator::new();
        let req = json!({"messages": [{"role": "user", "content": "hi"}]});
        let r = v.validate_pre_mapping(Dialect::OpenAi, &req);
        assert!(!r.is_valid());
        assert!(r.issues.iter().any(|i| i.field == "model"));
    }

    #[test]
    fn validation_missing_messages_openai() {
        let v = DefaultMappingValidator::new();
        let req = json!({"model": "gpt-4"});
        let r = v.validate_pre_mapping(Dialect::OpenAi, &req);
        assert!(!r.is_valid());
        assert!(r.issues.iter().any(|i| i.field == "messages"));
    }

    #[test]
    fn validation_missing_max_tokens_claude() {
        let v = DefaultMappingValidator::new();
        let req = json!({"model": "claude-3", "messages": [{"role": "user", "content": "hi"}]});
        let r = v.validate_pre_mapping(Dialect::Claude, &req);
        assert!(!r.is_valid());
        assert!(r.issues.iter().any(|i| i.field == "max_tokens"));
    }

    #[test]
    fn validation_missing_contents_gemini() {
        let v = DefaultMappingValidator::new();
        let req = json!({"model": "gemini-pro"});
        let r = v.validate_pre_mapping(Dialect::Gemini, &req);
        assert!(!r.is_valid());
        assert!(r.issues.iter().any(|i| i.field == "contents"));
    }

    #[test]
    fn validation_empty_messages_warning() {
        let v = DefaultMappingValidator::new();
        let req = json!({"model": "gpt-4", "messages": []});
        let r = v.validate_pre_mapping(Dialect::OpenAi, &req);
        assert!(r.is_valid()); // warning-only
        assert!(
            r.issues
                .iter()
                .any(|i| i.severity == ValidationSeverity::Warning)
        );
    }

    #[test]
    fn validation_non_object_request() {
        let v = DefaultMappingValidator::new();
        let r = v.validate_pre_mapping(Dialect::OpenAi, &json!("not_object"));
        assert!(!r.is_valid());
        assert!(r.issues.iter().any(|i| i.code == "invalid_type"));
    }

    #[test]
    fn validation_pipeline_pre_fail_blocks_mapping() {
        let pipe = ValidationPipeline::new(
            DefaultMappingValidator::new(),
            Dialect::OpenAi,
            Dialect::Claude,
        );
        let req = json!({"bad": true});
        let result = pipe.run(&req, |_| panic!("should not map"));
        assert!(!result.pre.is_valid());
        assert!(result.mapped.is_none());
    }

    #[test]
    fn validation_pipeline_map_error_captured() {
        let pipe = ValidationPipeline::new(
            DefaultMappingValidator::new(),
            Dialect::OpenAi,
            Dialect::Claude,
        );
        let req = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
        let result = pipe.run(&req, |_| Err("mapping failed".into()));
        assert!(result.pre.is_valid());
        assert!(result.mapped.is_none());
        let post = result.post.unwrap();
        assert!(!post.is_valid());
        assert_eq!(post.issues[0].code, "mapping_failed");
    }

    #[test]
    fn validation_full_coverage_openai() {
        let v = DefaultMappingValidator::new();
        let req = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
        let r = v.validate_pre_mapping(Dialect::OpenAi, &req);
        assert!(r.is_valid());
        assert_eq!(r.field_coverage, 100.0);
    }

    #[test]
    fn roundtrip_validation_detects_lost_fields() {
        let v = DefaultMappingValidator::new();
        let orig = json!({"model": "gpt-4", "messages": [], "logprobs": true, "n": 2});
        let rt = json!({"model": "gpt-4", "messages": [], "max_tokens": 4096});
        let r = v.validate_roundtrip(&orig, &rt);
        assert!(!r.is_lossless());
        assert!(r.lost_fields.iter().any(|f| f == "logprobs"));
        assert!(r.lost_fields.iter().any(|f| f == "n"));
    }

    #[test]
    fn roundtrip_validation_detects_added_fields() {
        let v = DefaultMappingValidator::new();
        let orig = json!({"model": "gpt-4"});
        let rt = json!({"model": "gpt-4", "max_tokens": 4096, "system": "help"});
        let r = v.validate_roundtrip(&orig, &rt);
        assert!(!r.is_lossless());
        assert!(r.added_fields.iter().any(|f| f == "max_tokens"));
    }

    #[test]
    fn roundtrip_validation_detects_changed_fields() {
        let v = DefaultMappingValidator::new();
        let orig = json!({"model": "gpt-4", "temperature": 0.7});
        let rt = json!({"model": "gpt-4", "temperature": 0.5});
        let r = v.validate_roundtrip(&orig, &rt);
        assert!(!r.is_lossless());
        assert!(r.changed_fields.contains(&"temperature".to_string()));
    }

    // ── Model name passthrough ──────────────────────────────────────────

    #[test]
    fn model_name_passed_through_openai_to_claude() {
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4o-mini",
                "messages": [{"role": "user", "content": "hi"}]
            }),
        };
        let result = OpenAiToClaudeMapper.map_request(&req).unwrap();
        assert_eq!(result["model"], "gpt-4o-mini");
    }

    #[test]
    fn model_name_passed_through_gemini_to_openai() {
        let req = DialectRequest {
            dialect: Dialect::Gemini,
            body: json!({
                "model": "gemini-1.5-pro",
                "contents": [{"role": "user", "parts": [{"text": "hi"}]}]
            }),
        };
        let result = GeminiToOpenAiMapper.map_request(&req).unwrap();
        assert_eq!(result["model"], "gemini-1.5-pro");
    }

    // ── Factory and supported pairs ─────────────────────────────────────

    #[test]
    fn factory_returns_mapper_for_all_supported_pairs() {
        use abp_mapper::{default_ir_mapper, supported_ir_pairs};
        let pairs = supported_ir_pairs();
        assert!(pairs.len() >= 24); // 6 identity + 18 cross-dialect
        for (from, to) in &pairs {
            let mapper = default_ir_mapper(*from, *to);
            assert!(mapper.is_some(), "no mapper for {from} -> {to}");
        }
    }

    #[test]
    fn factory_returns_none_for_unsupported_pair() {
        use abp_mapper::default_ir_mapper;
        // Codex ↔ Copilot has no direct mapper
        let mapper = default_ir_mapper(Dialect::Codex, Dialect::Copilot);
        assert!(mapper.is_none());
    }

    #[test]
    fn factory_identity_for_same_dialect() {
        use abp_mapper::default_ir_mapper;
        for d in Dialect::all() {
            let mapper = default_ir_mapper(*d, *d);
            assert!(mapper.is_some());
            let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "test")]);
            let result = mapper.unwrap().map_request(*d, *d, &conv).unwrap();
            assert_eq!(result, conv);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// § 7  ADDITIONAL FIDELITY TESTS
// ═══════════════════════════════════════════════════════════════════════════

mod additional_fidelity {
    use super::*;

    #[test]
    fn openai_stop_sequences_to_claude() {
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}],
                "stop": ["END", "DONE"]
            }),
        };
        let result = OpenAiToClaudeMapper.map_request(&req).unwrap();
        assert_eq!(result["stop_sequences"], json!(["END", "DONE"]));
    }

    #[test]
    fn openai_single_stop_string_to_claude_array() {
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}],
                "stop": "STOP"
            }),
        };
        let result = OpenAiToClaudeMapper.map_request(&req).unwrap();
        assert_eq!(result["stop_sequences"], json!(["STOP"]));
    }

    #[test]
    fn openai_stream_flag_to_claude() {
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}],
                "stream": true
            }),
        };
        let result = OpenAiToClaudeMapper.map_request(&req).unwrap();
        assert_eq!(result["stream"], true);
    }

    #[test]
    fn gemini_generation_config_roundtrip() {
        let openai_body = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 2048,
            "temperature": 0.7,
            "top_p": 0.9,
            "stop": ["END"]
        });
        let gemini = OpenAiToGeminiMapper
            .map_request(&DialectRequest {
                dialect: Dialect::OpenAi,
                body: openai_body,
            })
            .unwrap();
        let back = GeminiToOpenAiMapper
            .map_request(&DialectRequest {
                dialect: Dialect::Gemini,
                body: gemini,
            })
            .unwrap();
        assert_eq!(back["max_tokens"], 2048);
        assert_eq!(back["temperature"], 0.7);
        assert_eq!(back["top_p"], 0.9);
        assert_eq!(back["stop"], json!(["END"]));
    }

    #[test]
    fn openai_claude_roundtrip_system_message() {
        let openai_body = json!({
            "model": "gpt-4",
            "messages": [
                {"role": "system", "content": "You are a code reviewer"},
                {"role": "user", "content": "Review this"}
            ]
        });
        let claude = OpenAiToClaudeMapper
            .map_request(&DialectRequest {
                dialect: Dialect::OpenAi,
                body: openai_body,
            })
            .unwrap();
        assert_eq!(claude["system"], "You are a code reviewer");
        assert_eq!(claude["messages"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn openai_gemini_roundtrip_preserves_user_content() {
        let openai_body = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hello, world!"}]
        });
        let gemini = OpenAiToGeminiMapper
            .map_request(&DialectRequest {
                dialect: Dialect::OpenAi,
                body: openai_body,
            })
            .unwrap();
        let back = GeminiToOpenAiMapper
            .map_request(&DialectRequest {
                dialect: Dialect::Gemini,
                body: gemini,
            })
            .unwrap();
        assert_eq!(back["messages"][0]["content"], "Hello, world!");
    }

    #[test]
    fn default_max_tokens_added_for_claude() {
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}]
            }),
        };
        let result = OpenAiToClaudeMapper.map_request(&req).unwrap();
        assert_eq!(result["max_tokens"], 4096);
    }

    #[test]
    fn ir_codex_claude_bidirectional_supported_pairs() {
        let pairs = CodexClaudeIrMapper.supported_pairs();
        assert!(pairs.contains(&(Dialect::Codex, Dialect::Claude)));
        assert!(pairs.contains(&(Dialect::Claude, Dialect::Codex)));
    }

    #[test]
    fn ir_claude_to_codex_drops_system_and_tools() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "Be helpful"),
            IrMessage::text(IrRole::User, "Hello"),
            IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Text {
                        text: "answer".into(),
                    },
                    IrContentBlock::ToolUse {
                        id: "tu".into(),
                        name: "bash".into(),
                        input: json!({}),
                    },
                ],
            ),
            IrMessage::new(
                IrRole::Tool,
                vec![IrContentBlock::ToolResult {
                    tool_use_id: "tu".into(),
                    content: vec![],
                    is_error: false,
                }],
            ),
        ]);
        let result = CodexClaudeIrMapper
            .map_request(Dialect::Claude, Dialect::Codex, &conv)
            .unwrap();
        // System and Tool messages dropped, ToolUse block in assistant dropped
        assert!(result.messages.iter().all(|m| m.role != IrRole::System));
        assert!(result.messages.iter().all(|m| m.role != IrRole::Tool));
    }

    #[test]
    fn ir_openai_copilot_roundtrip_without_thinking() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "help"),
            IrMessage::text(IrRole::User, "hi"),
            IrMessage::text(IrRole::Assistant, "hello"),
        ]);
        let to_copilot = OpenAiCopilotIrMapper
            .map_request(Dialect::OpenAi, Dialect::Copilot, &conv)
            .unwrap();
        let back = OpenAiCopilotIrMapper
            .map_request(Dialect::Copilot, Dialect::OpenAi, &to_copilot)
            .unwrap();
        assert_eq!(back, conv);
    }

    #[test]
    fn ir_openai_kimi_roundtrip_without_thinking() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "hi"),
            IrMessage::text(IrRole::Assistant, "hello"),
        ]);
        let to_kimi = OpenAiKimiIrMapper
            .map_request(Dialect::OpenAi, Dialect::Kimi, &conv)
            .unwrap();
        let back = OpenAiKimiIrMapper
            .map_request(Dialect::Kimi, Dialect::OpenAi, &to_kimi)
            .unwrap();
        assert_eq!(back, conv);
    }

    #[test]
    fn mapping_error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MappingError>();
        assert_send_sync::<MapError>();
    }

    #[test]
    fn mapping_error_display_fidelity_loss() {
        let err = MappingError::FidelityLoss {
            field: "thinking".into(),
            source_dialect: Dialect::Claude,
            target_dialect: Dialect::OpenAi,
            detail: "no thinking block support".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("thinking"));
        assert!(msg.contains("fidelity loss"));
    }

    #[test]
    fn mapping_error_display_unsupported_capability() {
        let err = MappingError::UnsupportedCapability {
            capability: "logprobs".into(),
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
        };
        let msg = err.to_string();
        assert!(msg.contains("logprobs"));
        assert!(msg.contains("OpenAI"));
        assert!(msg.contains("Claude"));
    }

    #[test]
    fn map_error_serde_roundtrip_all_variants() {
        let errors = vec![
            MapError::UnsupportedPair {
                from: Dialect::OpenAi,
                to: Dialect::Claude,
            },
            MapError::LossyConversion {
                field: "thinking".into(),
                reason: "dropped".into(),
            },
            MapError::UnmappableTool {
                name: "apply_patch".into(),
                reason: "no equivalent".into(),
            },
            MapError::IncompatibleCapability {
                capability: "vision".into(),
                reason: "not supported".into(),
            },
            MapError::UnmappableContent {
                field: "system".into(),
                reason: "images in system".into(),
            },
        ];
        for err in &errors {
            let json = serde_json::to_string(err).unwrap();
            let back: MapError = serde_json::from_str(&json).unwrap();
            assert_eq!(*err, back);
        }
    }
}
