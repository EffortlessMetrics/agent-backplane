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
//! Snapshot tests for SDK type conversions to/from IR and WorkOrder/Receipt.
//!
//! Each module builds a representative SDK request, converts it through the
//! shim conversion layer, and snapshots the resulting IR / WorkOrder / response
//! JSON with `insta::assert_json_snapshot!`.

use abp_core::{AgentEvent, AgentEventKind, Outcome, ReceiptBuilder, UsageNormalized};
use chrono::{TimeZone, Utc};
use serde_json::json;
use uuid::Uuid;

/// Fixed UUID for deterministic snapshots.
fn fixed_uuid() -> Uuid {
    Uuid::parse_str("a1a2a3a4-b1b2-c1c2-d1d2-d3d4d5d6d7d8").unwrap()
}

/// Fixed timestamp for deterministic snapshots.
fn fixed_ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap()
}

/// Build a deterministic receipt with an assistant message trace event.
fn make_test_receipt(backend: &str) -> abp_core::Receipt {
    let ts = fixed_ts();
    ReceiptBuilder::new(backend)
        .outcome(Outcome::Complete)
        .work_order_id(fixed_uuid())
        .started_at(ts)
        .finished_at(ts + chrono::Duration::milliseconds(500))
        .usage(UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            ..Default::default()
        })
        .usage_raw(json!({
            "input_tokens": 100,
            "output_tokens": 50
        }))
        .add_trace_event(AgentEvent {
            ts,
            kind: AgentEventKind::AssistantMessage {
                text: "Hello! I can help with that.".into(),
            },
            ext: None,
        })
        .build()
}

/// Build a receipt with a tool call trace event.
fn make_tool_receipt(backend: &str) -> abp_core::Receipt {
    let ts = fixed_ts();
    ReceiptBuilder::new(backend)
        .outcome(Outcome::Complete)
        .work_order_id(fixed_uuid())
        .started_at(ts)
        .finished_at(ts + chrono::Duration::milliseconds(800))
        .usage(UsageNormalized {
            input_tokens: Some(150),
            output_tokens: Some(75),
            ..Default::default()
        })
        .add_trace_event(AgentEvent {
            ts,
            kind: AgentEventKind::ToolCall {
                tool_name: "get_weather".into(),
                tool_use_id: Some("call_abc123".into()),
                input: json!({"location": "San Francisco"}),
                parent_tool_use_id: None,
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts,
            kind: AgentEventKind::ToolResult {
                tool_name: "get_weather".into(),
                tool_use_id: Some("call_abc123".into()),
                output: json!("72°F and sunny"),
                is_error: false,
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts,
            kind: AgentEventKind::AssistantMessage {
                text: "The weather in San Francisco is 72°F and sunny.".into(),
            },
            ext: None,
        })
        .build()
}

// ── OpenAI ──────────────────────────────────────────────────────────────

mod openai {
    use super::*;
    use abp_shim_openai::convert;
    use abp_shim_openai::types::*;

    fn simple_request() -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![ChatMessage::User {
                content: MessageContent::Text("What is 2+2?".into()),
            }],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
        }
    }

    fn multi_turn_request() -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![
                ChatMessage::System {
                    content: "You are a helpful math tutor.".into(),
                },
                ChatMessage::User {
                    content: MessageContent::Text("What is 2+2?".into()),
                },
                ChatMessage::Assistant {
                    content: Some("2+2 equals 4.".into()),
                    tool_calls: None,
                },
                ChatMessage::User {
                    content: MessageContent::Text("What about 3+3?".into()),
                },
            ],
            temperature: Some(0.7),
            top_p: None,
            max_tokens: Some(1024),
            stream: None,
            tools: None,
            tool_choice: None,
        }
    }

    fn tool_request() -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![ChatMessage::User {
                content: MessageContent::Text("What's the weather in SF?".into()),
            }],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: Some(vec![Tool {
                tool_type: "function".into(),
                function: FunctionDef {
                    name: "get_weather".into(),
                    description: "Get current weather for a location".into(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "location": { "type": "string" }
                        },
                        "required": ["location"]
                    }),
                },
            }]),
            tool_choice: Some(ToolChoice::Mode(ToolChoiceMode::Auto)),
        }
    }

    fn system_request() -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![
                ChatMessage::System {
                    content: "You are a pirate. Always respond in pirate speak.".into(),
                },
                ChatMessage::User {
                    content: MessageContent::Text("Hello!".into()),
                },
            ],
            temperature: Some(1.0),
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
        }
    }

    #[test]
    fn simple_text_to_work_order() {
        let wo = convert::to_work_order(&simple_request());
        insta::assert_json_snapshot!("openai_simple_text_to_work_order", wo, {
            ".id" => "[uuid]",
        });
    }

    #[test]
    fn multi_turn_to_work_order() {
        let wo = convert::to_work_order(&multi_turn_request());
        insta::assert_json_snapshot!("openai_multi_turn_to_work_order", wo, {
            ".id" => "[uuid]",
        });
    }

    #[test]
    fn tool_use_to_work_order() {
        let wo = convert::to_work_order(&tool_request());
        insta::assert_json_snapshot!("openai_tool_use_to_work_order", wo, {
            ".id" => "[uuid]",
        });
    }

    #[test]
    fn system_message_to_work_order() {
        let wo = convert::to_work_order(&system_request());
        insta::assert_json_snapshot!("openai_system_message_to_work_order", wo, {
            ".id" => "[uuid]",
        });
    }

    #[test]
    fn receipt_to_response() {
        let wo = convert::to_work_order(&simple_request());
        let receipt = make_test_receipt("openai");
        let resp = convert::from_receipt(&receipt, &wo);
        insta::assert_json_snapshot!("openai_receipt_to_response", resp, {
            ".id" => "[id]",
            ".created" => "[timestamp]",
        });
    }

    #[test]
    fn tool_receipt_to_response() {
        let wo = convert::to_work_order(&tool_request());
        let receipt = make_tool_receipt("openai");
        let resp = convert::from_receipt(&receipt, &wo);
        insta::assert_json_snapshot!("openai_tool_receipt_to_response", resp, {
            ".id" => "[id]",
            ".created" => "[timestamp]",
        });
    }

    #[test]
    fn stream_event_assistant_delta() {
        let event = AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::AssistantDelta {
                text: "Hello".into(),
            },
            ext: None,
        };
        let chunk = convert::from_agent_event(&event, "gpt-4o", "chatcmpl-test");
        insta::assert_json_snapshot!("openai_stream_event_assistant_delta", chunk, {
            ".created" => "[timestamp]",
        });
    }
}

// ── Claude ──────────────────────────────────────────────────────────────

mod claude {
    use super::*;
    use abp_shim_claude::convert;
    use abp_shim_claude::types::*;

    fn simple_request() -> MessagesRequest {
        MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![ClaudeMessage {
                role: "user".into(),
                content: ClaudeContent::Text("What is 2+2?".into()),
            }],
            max_tokens: 1024,
            system: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stream: None,
            tools: None,
            tool_choice: None,
        }
    }

    fn multi_turn_request() -> MessagesRequest {
        MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![
                ClaudeMessage {
                    role: "user".into(),
                    content: ClaudeContent::Text("What is 2+2?".into()),
                },
                ClaudeMessage {
                    role: "assistant".into(),
                    content: ClaudeContent::Text("2+2 equals 4.".into()),
                },
                ClaudeMessage {
                    role: "user".into(),
                    content: ClaudeContent::Text("What about 3+3?".into()),
                },
            ],
            max_tokens: 1024,
            system: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stream: None,
            tools: None,
            tool_choice: None,
        }
    }

    fn tool_request() -> MessagesRequest {
        MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![ClaudeMessage {
                role: "user".into(),
                content: ClaudeContent::Text("What's the weather in SF?".into()),
            }],
            max_tokens: 1024,
            system: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stream: None,
            tools: Some(vec![ClaudeTool {
                name: "get_weather".into(),
                description: Some("Get current weather for a location".into()),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "location": { "type": "string" }
                    },
                    "required": ["location"]
                }),
            }]),
            tool_choice: Some(ClaudeToolChoice::Auto {}),
        }
    }

    fn system_request() -> MessagesRequest {
        MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![ClaudeMessage {
                role: "user".into(),
                content: ClaudeContent::Text("Hello!".into()),
            }],
            max_tokens: 1024,
            system: Some("You are a pirate. Always respond in pirate speak.".into()),
            temperature: Some(1.0),
            top_p: None,
            top_k: None,
            stream: None,
            tools: None,
            tool_choice: None,
        }
    }

    #[test]
    fn simple_text_to_work_order() {
        let wo = convert::to_work_order(&simple_request());
        insta::assert_json_snapshot!("claude_simple_text_to_work_order", wo, {
            ".id" => "[uuid]",
        });
    }

    #[test]
    fn multi_turn_to_work_order() {
        let wo = convert::to_work_order(&multi_turn_request());
        insta::assert_json_snapshot!("claude_multi_turn_to_work_order", wo, {
            ".id" => "[uuid]",
        });
    }

    #[test]
    fn tool_use_to_work_order() {
        let wo = convert::to_work_order(&tool_request());
        insta::assert_json_snapshot!("claude_tool_use_to_work_order", wo, {
            ".id" => "[uuid]",
        });
    }

    #[test]
    fn system_message_to_work_order() {
        let wo = convert::to_work_order(&system_request());
        insta::assert_json_snapshot!("claude_system_message_to_work_order", wo, {
            ".id" => "[uuid]",
        });
    }

    #[test]
    fn receipt_to_response() {
        let wo = convert::to_work_order(&simple_request());
        let receipt = make_test_receipt("claude");
        let resp = convert::from_receipt(&receipt, &wo);
        insta::assert_json_snapshot!("claude_receipt_to_response", resp, {
            ".id" => "[id]",
        });
    }

    #[test]
    fn tool_receipt_to_response() {
        let wo = convert::to_work_order(&tool_request());
        let receipt = make_tool_receipt("claude");
        let resp = convert::from_receipt(&receipt, &wo);
        insta::assert_json_snapshot!("claude_tool_receipt_to_response", resp, {
            ".id" => "[id]",
        });
    }

    #[test]
    fn stream_event_text_delta() {
        let event = AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::AssistantDelta {
                text: "Hello".into(),
            },
            ext: None,
        };
        let stream_evt = convert::from_agent_event(&event);
        insta::assert_json_snapshot!("claude_stream_event_text_delta", stream_evt);
    }
}

// ── Gemini ──────────────────────────────────────────────────────────────

mod gemini {
    use super::*;
    use abp_gemini_sdk::dialect::FunctionCallingMode;
    use abp_shim_gemini::convert;
    use abp_shim_gemini::types::*;

    fn simple_request() -> GenerateContentRequest {
        GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("What is 2+2?")]))
    }

    fn multi_turn_request() -> GenerateContentRequest {
        GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("What is 2+2?")]))
            .add_content(Content::model(vec![Part::text("2+2 equals 4.")]))
            .add_content(Content::user(vec![Part::text("What about 3+3?")]))
    }

    fn tool_request() -> GenerateContentRequest {
        GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("What's the weather in SF?")]))
            .tools(vec![ToolDeclaration {
                function_declarations: vec![FunctionDeclaration {
                    name: "get_weather".into(),
                    description: "Get current weather for a location".into(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "location": { "type": "string" }
                        },
                        "required": ["location"]
                    }),
                }],
            }])
            .tool_config(ToolConfig {
                function_calling_config: FunctionCallingConfig {
                    mode: FunctionCallingMode::Auto,
                    allowed_function_names: None,
                },
            })
    }

    fn system_request() -> GenerateContentRequest {
        GenerateContentRequest::new("gemini-2.5-flash")
            .system_instruction(Content {
                role: "user".into(),
                parts: vec![Part::text(
                    "You are a pirate. Always respond in pirate speak.",
                )],
            })
            .add_content(Content::user(vec![Part::text("Hello!")]))
    }

    #[test]
    fn simple_text_to_ir() {
        let (ir, _, _) = convert::request_to_ir(&simple_request()).unwrap();
        insta::assert_json_snapshot!("gemini_simple_text_to_ir", ir.conversation);
    }

    #[test]
    fn multi_turn_to_ir() {
        let (ir, _, _) = convert::request_to_ir(&multi_turn_request()).unwrap();
        insta::assert_json_snapshot!("gemini_multi_turn_to_ir", ir.conversation);
    }

    #[test]
    fn tool_use_to_ir() {
        let (ir, _, _) = convert::request_to_ir(&tool_request()).unwrap();
        insta::assert_json_snapshot!("gemini_tool_use_to_ir", ir.conversation);
    }

    #[test]
    fn system_message_to_ir() {
        let (ir, _, _) = convert::request_to_ir(&system_request()).unwrap();
        insta::assert_json_snapshot!("gemini_system_message_to_ir", ir.conversation);
    }

    #[test]
    fn simple_to_work_order() {
        let (ir, gen_config, _) = convert::request_to_ir(&simple_request()).unwrap();
        let wo = convert::ir_to_work_order(&ir, "gemini-2.5-flash", &gen_config);
        insta::assert_json_snapshot!("gemini_simple_to_work_order", wo, {
            ".id" => "[uuid]",
        });
    }

    #[test]
    fn receipt_to_ir_roundtrip() {
        let receipt = make_test_receipt("gemini");
        let ir = convert::receipt_to_ir(&receipt);
        insta::assert_json_snapshot!("gemini_receipt_to_ir_roundtrip", ir);
    }

    #[test]
    fn receipt_to_response() {
        let (ir_req, gen_config, safety) = convert::request_to_ir(&simple_request()).unwrap();
        let _wo = convert::ir_to_work_order(&ir_req, "gemini-2.5-flash", &gen_config);
        let receipt = make_test_receipt("gemini");
        let ir = convert::receipt_to_ir(&receipt);
        let resp = convert::ir_to_response(&ir, &receipt, &gen_config, &safety).unwrap();
        insta::assert_json_snapshot!("gemini_receipt_to_response", resp);
    }

    #[test]
    fn usage_ir_roundtrip() {
        let usage = UsageMetadata {
            prompt_token_count: 100,
            candidates_token_count: 50,
            total_token_count: 150,
        };
        let ir = convert::usage_to_ir(&usage);
        let back = convert::usage_from_ir(&ir);
        insta::assert_json_snapshot!("gemini_usage_ir_roundtrip", back);
    }
}

// ── Codex ───────────────────────────────────────────────────────────────

mod codex {
    use super::*;
    use abp_codex_sdk::dialect::{CodexFunctionDef, CodexInputItem, CodexRequest, CodexTool};
    use abp_shim_codex::convert;

    fn simple_request() -> CodexRequest {
        CodexRequest {
            model: "codex-mini-latest".into(),
            input: vec![CodexInputItem::Message {
                role: "user".into(),
                content: "What is 2+2?".into(),
            }],
            max_output_tokens: None,
            temperature: None,
            tools: vec![],
            text: None,
        }
    }

    fn multi_turn_request() -> CodexRequest {
        CodexRequest {
            model: "codex-mini-latest".into(),
            input: vec![
                CodexInputItem::Message {
                    role: "system".into(),
                    content: "You are a helpful assistant.".into(),
                },
                CodexInputItem::Message {
                    role: "user".into(),
                    content: "What is 2+2?".into(),
                },
                CodexInputItem::Message {
                    role: "assistant".into(),
                    content: "2+2 equals 4.".into(),
                },
                CodexInputItem::Message {
                    role: "user".into(),
                    content: "What about 3+3?".into(),
                },
            ],
            max_output_tokens: Some(1024),
            temperature: Some(0.7),
            tools: vec![],
            text: None,
        }
    }

    fn tool_request() -> CodexRequest {
        CodexRequest {
            model: "codex-mini-latest".into(),
            input: vec![CodexInputItem::Message {
                role: "user".into(),
                content: "What's the weather in SF?".into(),
            }],
            max_output_tokens: None,
            temperature: None,
            tools: vec![CodexTool::Function {
                function: CodexFunctionDef {
                    name: "get_weather".into(),
                    description: "Get current weather for a location".into(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "location": { "type": "string" }
                        },
                        "required": ["location"]
                    }),
                },
            }],
            text: None,
        }
    }

    fn system_request() -> CodexRequest {
        CodexRequest {
            model: "codex-mini-latest".into(),
            input: vec![
                CodexInputItem::Message {
                    role: "system".into(),
                    content: "You are a pirate. Always respond in pirate speak.".into(),
                },
                CodexInputItem::Message {
                    role: "user".into(),
                    content: "Hello!".into(),
                },
            ],
            max_output_tokens: None,
            temperature: None,
            tools: vec![],
            text: None,
        }
    }

    #[test]
    fn simple_text_to_ir() {
        let ir = convert::request_to_ir(&simple_request());
        insta::assert_json_snapshot!("codex_simple_text_to_ir", ir);
    }

    #[test]
    fn multi_turn_to_ir() {
        let ir = convert::request_to_ir(&multi_turn_request());
        insta::assert_json_snapshot!("codex_multi_turn_to_ir", ir);
    }

    #[test]
    fn tool_use_to_ir() {
        let ir = convert::request_to_ir(&tool_request());
        insta::assert_json_snapshot!("codex_tool_use_to_ir", ir);
    }

    #[test]
    fn system_message_to_ir() {
        let ir = convert::request_to_ir(&system_request());
        insta::assert_json_snapshot!("codex_system_message_to_ir", ir);
    }

    #[test]
    fn simple_to_work_order() {
        let wo = convert::request_to_work_order(&simple_request());
        insta::assert_json_snapshot!("codex_simple_to_work_order", wo, {
            ".id" => "[uuid]",
        });
    }

    #[test]
    fn receipt_to_response() {
        let receipt = make_test_receipt("codex");
        let resp = convert::receipt_to_response(&receipt, "codex-mini-latest");
        insta::assert_json_snapshot!("codex_receipt_to_response", resp, {
            ".id" => "[id]",
        });
    }

    #[test]
    fn tool_receipt_to_response() {
        let receipt = make_tool_receipt("codex");
        let resp = convert::receipt_to_response(&receipt, "codex-mini-latest");
        insta::assert_json_snapshot!("codex_tool_receipt_to_response", resp, {
            ".id" => "[id]",
        });
    }

    #[test]
    fn response_ir_roundtrip() {
        let receipt = make_test_receipt("codex");
        let resp = convert::receipt_to_response(&receipt, "codex-mini-latest");
        let ir = convert::response_to_ir(&resp);
        let items = convert::ir_to_response_items(&ir);
        insta::assert_json_snapshot!("codex_response_ir_roundtrip", items);
    }
}

// ── Copilot ─────────────────────────────────────────────────────────────

mod copilot {
    use super::*;
    use abp_copilot_sdk::dialect::CopilotRequest;
    use abp_shim_copilot::convert;
    use abp_shim_copilot::types::Message;

    fn simple_request() -> CopilotRequest {
        abp_shim_copilot::types::CopilotRequestBuilder::new()
            .model("gpt-4o")
            .messages(vec![Message::user("What is 2+2?")])
            .build()
    }

    fn multi_turn_request() -> CopilotRequest {
        abp_shim_copilot::types::CopilotRequestBuilder::new()
            .model("gpt-4o")
            .messages(vec![
                Message::system("You are a helpful math tutor."),
                Message::user("What is 2+2?"),
                Message::assistant("2+2 equals 4."),
                Message::user("What about 3+3?"),
            ])
            .build()
    }

    fn system_request() -> CopilotRequest {
        abp_shim_copilot::types::CopilotRequestBuilder::new()
            .model("gpt-4o")
            .messages(vec![
                Message::system("You are a pirate. Always respond in pirate speak."),
                Message::user("Hello!"),
            ])
            .build()
    }

    #[test]
    fn simple_text_to_ir() {
        let ir = convert::request_to_ir(&simple_request());
        insta::assert_json_snapshot!("copilot_simple_text_to_ir", ir);
    }

    #[test]
    fn multi_turn_to_ir() {
        let ir = convert::request_to_ir(&multi_turn_request());
        insta::assert_json_snapshot!("copilot_multi_turn_to_ir", ir);
    }

    #[test]
    fn system_message_to_ir() {
        let ir = convert::request_to_ir(&system_request());
        insta::assert_json_snapshot!("copilot_system_message_to_ir", ir);
    }

    #[test]
    fn simple_to_work_order() {
        let wo = convert::request_to_work_order(&simple_request());
        insta::assert_json_snapshot!("copilot_simple_to_work_order", wo, {
            ".id" => "[uuid]",
        });
    }

    #[test]
    fn receipt_to_response() {
        let receipt = make_test_receipt("copilot");
        let resp = convert::receipt_to_response(&receipt, "gpt-4o");
        insta::assert_json_snapshot!("copilot_receipt_to_response", resp);
    }

    #[test]
    fn tool_receipt_to_response() {
        let receipt = make_tool_receipt("copilot");
        let resp = convert::receipt_to_response(&receipt, "gpt-4o");
        insta::assert_json_snapshot!("copilot_tool_receipt_to_response", resp);
    }

    #[test]
    fn response_ir_roundtrip() {
        let receipt = make_test_receipt("copilot");
        let resp = convert::receipt_to_response(&receipt, "gpt-4o");
        let ir = convert::response_to_ir(&resp);
        let msgs = convert::ir_to_messages(&ir);
        insta::assert_json_snapshot!("copilot_response_ir_roundtrip", msgs);
    }

    #[test]
    fn messages_ir_roundtrip() {
        let msgs = vec![
            Message::system("Be helpful."),
            Message::user("Hello!"),
            Message::assistant("Hi there!"),
        ];
        let ir = convert::messages_to_ir(&msgs);
        let back = convert::ir_to_messages(&ir);
        insta::assert_json_snapshot!("copilot_messages_ir_roundtrip", back);
    }
}

// ── Kimi ────────────────────────────────────────────────────────────────

mod kimi {
    use super::*;
    use abp_kimi_sdk::dialect::{KimiFunctionDef, KimiTool};
    use abp_shim_kimi::convert;
    use abp_shim_kimi::types::{KimiRequestBuilder, Message};

    fn simple_request() -> abp_kimi_sdk::dialect::KimiRequest {
        KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![Message::user("What is 2+2?")])
            .build()
    }

    fn multi_turn_request() -> abp_kimi_sdk::dialect::KimiRequest {
        KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![
                Message::system("You are a helpful math tutor."),
                Message::user("What is 2+2?"),
                Message::assistant("2+2 equals 4."),
                Message::user("What about 3+3?"),
            ])
            .max_tokens(1024)
            .temperature(0.7)
            .build()
    }

    fn tool_request() -> abp_kimi_sdk::dialect::KimiRequest {
        KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![Message::user("What's the weather in SF?")])
            .tools(vec![KimiTool::Function {
                function: KimiFunctionDef {
                    name: "get_weather".into(),
                    description: "Get current weather for a location".into(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "location": { "type": "string" }
                        },
                        "required": ["location"]
                    }),
                },
            }])
            .build()
    }

    fn system_request() -> abp_kimi_sdk::dialect::KimiRequest {
        KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![
                Message::system("You are a pirate. Always respond in pirate speak."),
                Message::user("Hello!"),
            ])
            .temperature(1.0)
            .build()
    }

    #[test]
    fn simple_text_to_ir() {
        let ir = convert::request_to_ir(&simple_request());
        insta::assert_json_snapshot!("kimi_simple_text_to_ir", ir);
    }

    #[test]
    fn multi_turn_to_ir() {
        let ir = convert::request_to_ir(&multi_turn_request());
        insta::assert_json_snapshot!("kimi_multi_turn_to_ir", ir);
    }

    #[test]
    fn tool_use_to_ir() {
        let ir = convert::request_to_ir(&tool_request());
        insta::assert_json_snapshot!("kimi_tool_use_to_ir", ir);
    }

    #[test]
    fn system_message_to_ir() {
        let ir = convert::request_to_ir(&system_request());
        insta::assert_json_snapshot!("kimi_system_message_to_ir", ir);
    }

    #[test]
    fn simple_to_work_order() {
        let wo = convert::request_to_work_order(&simple_request());
        insta::assert_json_snapshot!("kimi_simple_to_work_order", wo, {
            ".id" => "[uuid]",
        });
    }

    #[test]
    fn receipt_to_response() {
        let receipt = make_test_receipt("kimi");
        let resp = convert::receipt_to_response(&receipt, "moonshot-v1-8k");
        insta::assert_json_snapshot!("kimi_receipt_to_response", resp, {
            ".id" => "[id]",
        });
    }

    #[test]
    fn tool_receipt_to_response() {
        let receipt = make_tool_receipt("kimi");
        let resp = convert::receipt_to_response(&receipt, "moonshot-v1-8k");
        insta::assert_json_snapshot!("kimi_tool_receipt_to_response", resp, {
            ".id" => "[id]",
        });
    }

    #[test]
    fn response_ir_roundtrip() {
        let receipt = make_test_receipt("kimi");
        let resp = convert::receipt_to_response(&receipt, "moonshot-v1-8k");
        let ir = convert::response_to_ir(&resp);
        let msgs = convert::ir_to_messages(&ir);
        insta::assert_json_snapshot!("kimi_response_ir_roundtrip", msgs);
    }

    #[test]
    fn messages_ir_roundtrip() {
        let msgs = vec![
            Message::system("Be helpful."),
            Message::user("Hello!"),
            Message::assistant("Hi there!"),
        ];
        let ir = convert::messages_to_ir(&msgs);
        let back = convert::ir_to_messages(&ir);
        insta::assert_json_snapshot!("kimi_messages_ir_roundtrip", back);
    }

    #[test]
    fn usage_ir_roundtrip() {
        let ir_usage = abp_core::ir::IrUsage::from_io(100, 50);
        let usage = convert::ir_usage_to_usage(&ir_usage);
        insta::assert_json_snapshot!("kimi_usage_ir_roundtrip", usage);
    }
}
