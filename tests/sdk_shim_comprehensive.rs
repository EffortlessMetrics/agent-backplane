#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
//! Comprehensive tests for the SDK shim crates.
//!
//! Tests each vendor SDK adapter's public API surface including:
//! - WorkOrder creation from vendor request types
//! - Receipt mapping back to vendor response types
//! - Streaming chunk/event mapping
//! - Error code mapping
//! - Capability negotiation
//! - Passthrough vs mapped mode
//! - Tool definition translation
//! - Model name canonicalization

use abp_core::*;
use chrono::Utc;
use serde_json::json;
use std::collections::BTreeMap;

/// Helper to compare SupportLevel variants (SupportLevel does not derive PartialEq
/// due to the Restricted { reason } variant).
macro_rules! assert_support_level {
    ($actual:expr, Native) => {
        assert!(
            matches!($actual, Some(SupportLevel::Native)),
            "expected Native, got {:?}",
            $actual
        );
    };
    ($actual:expr, Emulated) => {
        assert!(
            matches!($actual, Some(SupportLevel::Emulated)),
            "expected Emulated, got {:?}",
            $actual
        );
    };
    ($actual:expr, Unsupported) => {
        assert!(
            matches!($actual, Some(SupportLevel::Unsupported)),
            "expected Unsupported, got {:?}",
            $actual
        );
    };
}

// ============================================================================
// Helpers
// ============================================================================

fn make_receipt_with_text(text: &str) -> Receipt {
    ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: text.to_string(),
            },
            ext: None,
        })
        .build()
}

fn make_receipt_with_tool_call(name: &str, id: &str, input: serde_json::Value) -> Receipt {
    ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: name.to_string(),
                tool_use_id: Some(id.to_string()),
                parent_tool_use_id: None,
                input,
            },
            ext: None,
        })
        .build()
}

fn make_receipt_failed() -> Receipt {
    ReceiptBuilder::new("mock")
        .outcome(Outcome::Failed)
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error {
                message: "something went wrong".into(),
                error_code: Some(abp_error::ErrorCode::Internal),
            },
            ext: None,
        })
        .build()
}

fn make_receipt_partial() -> Receipt {
    ReceiptBuilder::new("mock")
        .outcome(Outcome::Partial)
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "partial".into(),
            },
            ext: None,
        })
        .build()
}

fn make_receipt_with_usage(input_tokens: u64, output_tokens: u64) -> Receipt {
    ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .usage(UsageNormalized {
            input_tokens: Some(input_tokens),
            output_tokens: Some(output_tokens),
            cache_read_tokens: None,
            cache_write_tokens: None,
            request_units: None,
            estimated_cost_usd: None,
        })
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "hello".into(),
            },
            ext: None,
        })
        .build()
}

fn make_receipt_empty() -> Receipt {
    ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
}

fn sample_tool_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "path": { "type": "string" }
        },
        "required": ["path"]
    })
}

// ============================================================================
// OpenAI SDK Tests
// ============================================================================
mod openai {
    use super::*;
    use abp_openai_sdk::api::*;
    use abp_openai_sdk::dialect;
    use abp_openai_sdk::response_format::ResponseFormat;
    use abp_openai_sdk::streaming::*;
    use abp_openai_sdk::validation::*;

    // -- Constants --

    #[test]
    fn backend_name_is_correct() {
        assert_eq!(abp_openai_sdk::BACKEND_NAME, "sidecar:openai");
    }

    #[test]
    fn host_script_relative_path() {
        assert_eq!(abp_openai_sdk::HOST_SCRIPT_RELATIVE, "hosts/openai/host.js");
    }

    #[test]
    fn default_node_command() {
        assert_eq!(abp_openai_sdk::DEFAULT_NODE_COMMAND, "node");
    }

    // -- Model name mapping --

    #[test]
    fn to_canonical_model_adds_prefix() {
        assert_eq!(dialect::to_canonical_model("gpt-4o"), "openai/gpt-4o");
    }

    #[test]
    fn from_canonical_model_strips_prefix() {
        assert_eq!(dialect::from_canonical_model("openai/gpt-4o"), "gpt-4o");
    }

    #[test]
    fn from_canonical_model_passthrough_unknown() {
        assert_eq!(dialect::from_canonical_model("other/model"), "other/model");
    }

    #[test]
    fn is_known_model_true_for_gpt4o() {
        assert!(dialect::is_known_model("gpt-4o"));
    }

    #[test]
    fn is_known_model_false_for_unknown() {
        assert!(!dialect::is_known_model("gpt-99"));
    }

    #[test]
    fn canonical_roundtrip() {
        let model = "gpt-4o";
        let canonical = dialect::to_canonical_model(model);
        let back = dialect::from_canonical_model(&canonical);
        assert_eq!(back, model);
    }

    // -- Capability manifest --

    #[test]
    fn capability_manifest_has_streaming() {
        let m = dialect::capability_manifest();
        assert_support_level!(m.get(&Capability::Streaming), Native);
    }

    #[test]
    fn capability_manifest_mcp_unsupported() {
        let m = dialect::capability_manifest();
        assert_support_level!(m.get(&Capability::McpClient), Unsupported);
    }

    #[test]
    fn capability_manifest_structured_output() {
        let m = dialect::capability_manifest();
        assert_support_level!(m.get(&Capability::StructuredOutputJsonSchema), Native);
    }

    #[test]
    fn capability_manifest_tool_read_emulated() {
        let m = dialect::capability_manifest();
        assert_support_level!(m.get(&Capability::ToolRead), Emulated);
    }

    // -- WorkOrder creation --

    #[test]
    fn request_to_work_order_extracts_task() {
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![Message::User {
                content: "Hello world".into(),
            }],
            temperature: None,
            max_tokens: None,
            tools: None,
            tool_choice: None,
            stream: None,
            top_p: None,
            frequency_penalty: None,
            presence_penalty: None,
            stop: None,
            n: None,
            seed: None,
            response_format: None,
            user: None,
        };
        let wo: WorkOrder = req.into();
        assert_eq!(wo.task, "Hello world");
    }

    #[test]
    fn request_to_work_order_uses_last_user_message() {
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![
                Message::User {
                    content: "first".into(),
                },
                Message::User {
                    content: "second".into(),
                },
            ],
            temperature: None,
            max_tokens: None,
            tools: None,
            tool_choice: None,
            stream: None,
            top_p: None,
            frequency_penalty: None,
            presence_penalty: None,
            stop: None,
            n: None,
            seed: None,
            response_format: None,
            user: None,
        };
        let wo: WorkOrder = req.into();
        assert_eq!(wo.task, "second");
    }

    #[test]
    fn request_to_work_order_system_messages_as_snippets() {
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![
                Message::System {
                    content: "You are helpful".into(),
                },
                Message::User {
                    content: "hi".into(),
                },
            ],
            temperature: None,
            max_tokens: None,
            tools: None,
            tool_choice: None,
            stream: None,
            top_p: None,
            frequency_penalty: None,
            presence_penalty: None,
            stop: None,
            n: None,
            seed: None,
            response_format: None,
            user: None,
        };
        let wo: WorkOrder = req.into();
        assert!(!wo.context.snippets.is_empty());
        assert_eq!(wo.context.snippets[0].content, "You are helpful");
    }

    #[test]
    fn request_to_work_order_sets_model() {
        let req = ChatCompletionRequest {
            model: "gpt-4-turbo".into(),
            messages: vec![Message::User {
                content: "hi".into(),
            }],
            temperature: None,
            max_tokens: None,
            tools: None,
            tool_choice: None,
            stream: None,
            top_p: None,
            frequency_penalty: None,
            presence_penalty: None,
            stop: None,
            n: None,
            seed: None,
            response_format: None,
            user: None,
        };
        let wo: WorkOrder = req.into();
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4-turbo"));
    }

    #[test]
    fn request_to_work_order_no_user_message_yields_empty_task() {
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![Message::System {
                content: "You are a bot".into(),
            }],
            temperature: None,
            max_tokens: None,
            tools: None,
            tool_choice: None,
            stream: None,
            top_p: None,
            frequency_penalty: None,
            presence_penalty: None,
            stop: None,
            n: None,
            seed: None,
            response_format: None,
            user: None,
        };
        let wo: WorkOrder = req.into();
        assert_eq!(wo.task, "");
    }

    // -- Receipt to Response --

    #[test]
    fn receipt_to_response_maps_text() {
        let receipt = make_receipt_with_text("Hello from GPT");
        let resp: ChatCompletionResponse = receipt.into();
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Hello from GPT")
        );
    }

    #[test]
    fn receipt_to_response_stop_finish_reason() {
        let receipt = make_receipt_with_text("done");
        let resp: ChatCompletionResponse = receipt.into();
        assert_eq!(resp.choices[0].finish_reason, FinishReason::Stop);
    }

    #[test]
    fn receipt_to_response_tool_calls_finish_reason() {
        let receipt = make_receipt_with_tool_call("read_file", "call_0", json!({"path": "a.txt"}));
        let resp: ChatCompletionResponse = receipt.into();
        assert_eq!(resp.choices[0].finish_reason, FinishReason::ToolCalls);
        let tc = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tc[0].function.name, "read_file");
        assert_eq!(tc[0].id, "call_0");
    }

    #[test]
    fn receipt_to_response_object_type() {
        let receipt = make_receipt_with_text("test");
        let resp: ChatCompletionResponse = receipt.into();
        assert_eq!(resp.object, "chat.completion");
    }

    #[test]
    fn receipt_to_response_model_field() {
        let receipt = make_receipt_with_text("test");
        let resp: ChatCompletionResponse = receipt.into();
        // model comes from backend identity or config
        assert!(!resp.model.is_empty());
    }

    #[test]
    fn receipt_to_response_failed_outcome() {
        let receipt = make_receipt_failed();
        let resp: ChatCompletionResponse = receipt.into();
        assert_eq!(resp.choices[0].finish_reason, FinishReason::Stop);
    }

    #[test]
    fn receipt_to_response_empty_trace() {
        let receipt = make_receipt_empty();
        let resp: ChatCompletionResponse = receipt.into();
        assert_eq!(resp.choices.len(), 1);
        assert!(resp.choices[0].message.content.is_none());
    }

    // -- Tool definition translation --

    #[test]
    fn tool_def_to_openai_roundtrip() {
        let canonical = dialect::CanonicalToolDef {
            name: "read_file".into(),
            description: "Read a file".into(),
            parameters_schema: sample_tool_schema(),
        };
        let openai = dialect::tool_def_to_openai(&canonical);
        assert_eq!(openai.function.name, "read_file");
        assert_eq!(openai.tool_type, "function");

        let back = dialect::tool_def_from_openai(&openai);
        assert_eq!(back.name, canonical.name);
        assert_eq!(back.description, canonical.description);
    }

    // -- Response format --

    #[test]
    fn response_format_text() {
        let fmt = ResponseFormat::text();
        let json = serde_json::to_value(&fmt).unwrap();
        assert_eq!(json["type"], "text");
    }

    #[test]
    fn response_format_json_object() {
        let fmt = ResponseFormat::json_object();
        let json = serde_json::to_value(&fmt).unwrap();
        assert_eq!(json["type"], "json_object");
    }

    #[test]
    fn response_format_json_schema() {
        let fmt = ResponseFormat::json_schema("my_schema", json!({"type": "object"}));
        let json = serde_json::to_value(&fmt).unwrap();
        assert_eq!(json["type"], "json_schema");
        assert_eq!(json["json_schema"]["name"], "my_schema");
    }

    #[test]
    fn response_format_serde_roundtrip() {
        let fmt = ResponseFormat::json_schema("test", json!({"type": "string"}));
        let serialized = serde_json::to_string(&fmt).unwrap();
        let deserialized: ResponseFormat = serde_json::from_str(&serialized).unwrap();
        assert_eq!(fmt, deserialized);
    }

    // -- Streaming --

    #[test]
    fn map_chunk_text_delta() {
        let chunk = ChatCompletionChunk {
            id: "chunk1".into(),
            object: "chat.completion.chunk".into(),
            created: 1234567890,
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
        let events = map_chunk(&chunk);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::AssistantDelta { text } => assert_eq!(text, "Hello"),
            other => panic!("expected AssistantDelta, got {:?}", other),
        }
    }

    #[test]
    fn map_chunk_empty_content_no_event() {
        let chunk = ChatCompletionChunk {
            id: "chunk2".into(),
            object: "chat.completion.chunk".into(),
            created: 1234567890,
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
        let events = map_chunk(&chunk);
        assert!(events.is_empty());
    }

    #[test]
    fn tool_call_accumulator_single_call() {
        let mut acc = ToolCallAccumulator::new();
        acc.feed(&[ChunkToolCall {
            index: 0,
            id: Some("call_1".into()),
            call_type: Some("function".into()),
            function: Some(ChunkFunctionCall {
                name: Some("read_file".into()),
                arguments: Some(r#"{"path":"#.into()),
            }),
        }]);
        acc.feed(&[ChunkToolCall {
            index: 0,
            id: None,
            call_type: None,
            function: Some(ChunkFunctionCall {
                name: None,
                arguments: Some(r#""test.txt"}"#.into()),
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
                assert_eq!(tool_name, "read_file");
                assert_eq!(tool_use_id.as_deref(), Some("call_1"));
                assert_eq!(input["path"], "test.txt");
            }
            other => panic!("expected ToolCall, got {:?}", other),
        }
    }

    #[test]
    fn tool_call_accumulator_finish_as_openai() {
        let mut acc = ToolCallAccumulator::new();
        acc.feed(&[ChunkToolCall {
            index: 0,
            id: Some("call_2".into()),
            call_type: Some("function".into()),
            function: Some(ChunkFunctionCall {
                name: Some("write_file".into()),
                arguments: Some(r#"{"content":"hi"}"#.into()),
            }),
        }]);
        let pairs = acc.finish_as_openai();
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].0, "call_2");
        assert_eq!(pairs[0].1.name, "write_file");
    }

    #[test]
    fn tool_call_accumulator_multiple_calls() {
        let mut acc = ToolCallAccumulator::new();
        acc.feed(&[
            ChunkToolCall {
                index: 0,
                id: Some("call_a".into()),
                call_type: Some("function".into()),
                function: Some(ChunkFunctionCall {
                    name: Some("func_a".into()),
                    arguments: Some("{}".into()),
                }),
            },
            ChunkToolCall {
                index: 1,
                id: Some("call_b".into()),
                call_type: Some("function".into()),
                function: Some(ChunkFunctionCall {
                    name: Some("func_b".into()),
                    arguments: Some("{}".into()),
                }),
            },
        ]);
        let events = acc.finish();
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn tool_call_accumulator_empty() {
        let acc = ToolCallAccumulator::new();
        let events = acc.finish();
        assert!(events.is_empty());
    }

    // -- Validation --

    #[test]
    fn validate_no_extended_fields_ok() {
        let fields = ExtendedRequestFields::default();
        assert!(validate_for_mapped_mode(&fields).is_ok());
    }

    #[test]
    fn validate_logprobs_fails() {
        let fields = ExtendedRequestFields {
            logprobs: Some(true),
            ..Default::default()
        };
        let err = validate_for_mapped_mode(&fields).unwrap_err();
        assert!(err.errors.iter().any(|e| e.param == "logprobs"));
    }

    #[test]
    fn validate_seed_fails() {
        let fields = ExtendedRequestFields {
            seed: Some(42),
            ..Default::default()
        };
        let err = validate_for_mapped_mode(&fields).unwrap_err();
        assert!(err.errors.iter().any(|e| e.param == "seed"));
    }

    #[test]
    fn validate_logit_bias_fails() {
        let mut bias = BTreeMap::new();
        bias.insert("123".into(), 1.0);
        let fields = ExtendedRequestFields {
            logit_bias: Some(bias),
            ..Default::default()
        };
        let err = validate_for_mapped_mode(&fields).unwrap_err();
        assert!(err.errors.iter().any(|e| e.param == "logit_bias"));
    }

    #[test]
    fn validate_multiple_errors() {
        let fields = ExtendedRequestFields {
            logprobs: Some(true),
            seed: Some(42),
            ..Default::default()
        };
        let err = validate_for_mapped_mode(&fields).unwrap_err();
        assert!(err.errors.len() >= 2);
    }

    #[test]
    fn validation_errors_display() {
        let errs = ValidationErrors {
            errors: vec![UnmappableParam {
                param: "seed".into(),
                reason: "not supported".into(),
            }],
        };
        let display = format!("{}", errs);
        assert!(display.contains("1 unmappable parameter"));
    }

    // -- Serde roundtrips --

    #[test]
    fn message_serde_user() {
        let msg = Message::User {
            content: "hi".into(),
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["role"], "user");
        let back: Message = serde_json::from_value(json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn message_serde_system() {
        let msg = Message::System {
            content: "You are helpful".into(),
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["role"], "system");
    }

    #[test]
    fn message_serde_assistant_with_tool_calls() {
        let msg = Message::Assistant {
            content: None,
            tool_calls: Some(vec![ToolCall {
                id: "tc_1".into(),
                call_type: "function".into(),
                function: FunctionCall {
                    name: "grep".into(),
                    arguments: r#"{"q":"hello"}"#.into(),
                },
            }]),
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["role"], "assistant");
        assert!(json["tool_calls"].is_array());
    }

    #[test]
    fn message_serde_tool() {
        let msg = Message::Tool {
            tool_call_id: "tc_1".into(),
            content: "result".into(),
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["role"], "tool");
    }

    #[test]
    fn finish_reason_serde() {
        let json = serde_json::to_value(&FinishReason::ToolCalls).unwrap();
        assert_eq!(json, "tool_calls");
        let back: FinishReason = serde_json::from_value(json).unwrap();
        assert_eq!(back, FinishReason::ToolCalls);
    }

    // -- Dialect version --

    #[test]
    fn dialect_version_constant() {
        assert_eq!(dialect::DIALECT_VERSION, "openai/v0.1");
    }

    #[test]
    fn default_model_constant() {
        assert_eq!(dialect::DEFAULT_MODEL, "gpt-4o");
    }

    #[test]
    fn sidecar_script_path() {
        let root = std::path::Path::new("/fake");
        let script = abp_openai_sdk::sidecar_script(root);
        assert_eq!(script, root.join("hosts/openai/host.js"));
    }
}

// ============================================================================
// Claude SDK Tests
// ============================================================================
mod claude {
    use super::*;
    use abp_claude_sdk::dialect;
    use abp_claude_sdk::messages::*;

    // -- Constants --

    #[test]
    fn backend_name_is_correct() {
        assert_eq!(abp_claude_sdk::BACKEND_NAME, "sidecar:claude");
    }

    #[test]
    fn host_script_relative_path() {
        assert_eq!(abp_claude_sdk::HOST_SCRIPT_RELATIVE, "hosts/claude/host.js");
    }

    // -- Model mapping --

    #[test]
    fn to_canonical_model_adds_prefix() {
        assert_eq!(
            dialect::to_canonical_model("claude-sonnet-4-20250514"),
            "anthropic/claude-sonnet-4-20250514"
        );
    }

    #[test]
    fn from_canonical_model_strips_prefix() {
        assert_eq!(
            dialect::from_canonical_model("anthropic/claude-sonnet-4-20250514"),
            "claude-sonnet-4-20250514"
        );
    }

    #[test]
    fn from_canonical_model_passthrough() {
        assert_eq!(
            dialect::from_canonical_model("openai/gpt-4"),
            "openai/gpt-4"
        );
    }

    #[test]
    fn is_known_model_claude_sonnet() {
        assert!(dialect::is_known_model("claude-sonnet-4-20250514"));
    }

    #[test]
    fn is_known_model_false_for_unknown() {
        assert!(!dialect::is_known_model("claude-99"));
    }

    #[test]
    fn canonical_roundtrip() {
        let model = "claude-opus-4-20250514";
        let canonical = dialect::to_canonical_model(model);
        let back = dialect::from_canonical_model(&canonical);
        assert_eq!(back, model);
    }

    // -- Capability manifest --

    #[test]
    fn capability_manifest_streaming_native() {
        let m = dialect::capability_manifest();
        assert_support_level!(m.get(&Capability::Streaming), Native);
    }

    #[test]
    fn capability_manifest_tool_read_native() {
        let m = dialect::capability_manifest();
        assert_support_level!(m.get(&Capability::ToolRead), Native);
    }

    #[test]
    fn capability_manifest_mcp_client_native() {
        let m = dialect::capability_manifest();
        assert_support_level!(m.get(&Capability::McpClient), Native);
    }

    #[test]
    fn capability_manifest_mcp_server_unsupported() {
        let m = dialect::capability_manifest();
        assert_support_level!(m.get(&Capability::McpServer), Unsupported);
    }

    #[test]
    fn capability_manifest_web_search_native() {
        let m = dialect::capability_manifest();
        assert_support_level!(m.get(&Capability::ToolWebSearch), Native);
    }

    // -- WorkOrder creation --

    #[test]
    fn messages_request_to_work_order_extracts_task() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![Message {
                role: Role::User,
                content: MessageContent::Text("Hello Claude".into()),
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
        };
        let wo: WorkOrder = req.into();
        assert_eq!(wo.task, "Hello Claude");
    }

    #[test]
    fn messages_request_with_system_prompt() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![Message {
                role: Role::User,
                content: MessageContent::Text("hi".into()),
            }],
            max_tokens: 1024,
            system: Some(SystemMessage::Text("You are helpful".into())),
            tools: None,
            metadata: None,
            stream: None,
            stop_sequences: None,
            temperature: None,
            top_p: None,
            top_k: None,
        };
        let wo: WorkOrder = req.into();
        // System prompt stored in vendor config, not snippets
        let sys = wo.config.vendor.get("system").unwrap();
        assert_eq!(sys.as_str().unwrap(), "You are helpful");
    }

    #[test]
    fn messages_request_sets_model() {
        let req = MessagesRequest {
            model: "claude-opus-4-20250514".into(),
            messages: vec![Message {
                role: Role::User,
                content: MessageContent::Text("hi".into()),
            }],
            max_tokens: 2048,
            system: None,
            tools: None,
            metadata: None,
            stream: None,
            stop_sequences: None,
            temperature: None,
            top_p: None,
            top_k: None,
        };
        let wo: WorkOrder = req.into();
        assert_eq!(wo.config.model.as_deref(), Some("claude-opus-4-20250514"));
    }

    // -- Receipt to Response --

    #[test]
    fn receipt_to_response_maps_text() {
        let receipt = make_receipt_with_text("Hello from Claude");
        let resp: MessagesResponse = receipt.into();
        assert_eq!(resp.role, "assistant");
        assert!(!resp.content.is_empty());
        match &resp.content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "Hello from Claude"),
            other => panic!("expected Text block, got {:?}", other),
        }
    }

    #[test]
    fn receipt_to_response_object_type() {
        let receipt = make_receipt_with_text("test");
        let resp: MessagesResponse = receipt.into();
        assert_eq!(resp.response_type, "message");
    }

    #[test]
    fn receipt_to_response_stop_reason() {
        let receipt = make_receipt_with_text("done");
        let resp: MessagesResponse = receipt.into();
        assert_eq!(resp.stop_reason, Some("end_turn".into()));
    }

    #[test]
    fn receipt_to_response_tool_use() {
        let receipt = make_receipt_with_tool_call("read_file", "tu_01", json!({"path": "a.txt"}));
        let resp: MessagesResponse = receipt.into();
        let has_tool_use = resp
            .content
            .iter()
            .any(|b| matches!(b, ContentBlock::ToolUse { name, .. } if name == "read_file"));
        assert!(has_tool_use);
    }

    #[test]
    fn receipt_to_response_failed_outcome() {
        let receipt = make_receipt_failed();
        let resp: MessagesResponse = receipt.into();
        // Failed receipts should still produce a response
        assert_eq!(resp.response_type, "message");
    }

    // -- Tool definition translation --

    #[test]
    fn tool_def_to_claude_roundtrip() {
        let canonical = dialect::CanonicalToolDef {
            name: "grep".into(),
            description: "Search for patterns".into(),
            parameters_schema: sample_tool_schema(),
        };
        let claude = dialect::tool_def_to_claude(&canonical);
        assert_eq!(claude.name, "grep");
        assert_eq!(claude.input_schema, canonical.parameters_schema);

        let back = dialect::tool_def_from_claude(&claude);
        assert_eq!(back.name, canonical.name);
        assert_eq!(back.description, canonical.description);
    }

    // -- Serde --

    #[test]
    fn role_serde_roundtrip() {
        let json = serde_json::to_value(&Role::User).unwrap();
        assert_eq!(json, "user");
        let back: Role = serde_json::from_value(json).unwrap();
        assert_eq!(back, Role::User);
    }

    #[test]
    fn role_serde_assistant() {
        let json = serde_json::to_value(&Role::Assistant).unwrap();
        assert_eq!(json, "assistant");
    }

    #[test]
    fn message_content_text_serde() {
        let content = MessageContent::Text("hello".into());
        let json = serde_json::to_value(&content).unwrap();
        assert_eq!(json, "hello");
    }

    #[test]
    fn dialect_version_constant() {
        assert_eq!(dialect::DIALECT_VERSION, "claude/v0.1");
    }

    #[test]
    fn default_model_constant() {
        assert_eq!(dialect::DEFAULT_MODEL, "claude-sonnet-4-20250514");
    }

    #[test]
    fn sidecar_script_path() {
        let root = std::path::Path::new("/fake");
        let script = abp_claude_sdk::sidecar_script(root);
        assert_eq!(script, root.join("hosts/claude/host.js"));
    }
}

// ============================================================================
// Gemini SDK Tests
// ============================================================================
mod gemini {
    use super::*;
    use abp_gemini_sdk::dialect;
    use abp_gemini_sdk::types::*;

    // -- Constants --

    #[test]
    fn backend_name_is_correct() {
        assert_eq!(abp_gemini_sdk::BACKEND_NAME, "sidecar:gemini");
    }

    #[test]
    fn host_script_relative_path() {
        assert_eq!(abp_gemini_sdk::HOST_SCRIPT_RELATIVE, "hosts/gemini/host.js");
    }

    // -- Model mapping --

    #[test]
    fn to_canonical_model_adds_prefix() {
        assert_eq!(
            dialect::to_canonical_model("gemini-2.5-flash"),
            "google/gemini-2.5-flash"
        );
    }

    #[test]
    fn from_canonical_model_strips_prefix() {
        assert_eq!(
            dialect::from_canonical_model("google/gemini-2.5-flash"),
            "gemini-2.5-flash"
        );
    }

    #[test]
    fn from_canonical_model_passthrough() {
        assert_eq!(
            dialect::from_canonical_model("openai/gpt-4o"),
            "openai/gpt-4o"
        );
    }

    #[test]
    fn is_known_model_gemini_flash() {
        assert!(dialect::is_known_model("gemini-2.5-flash"));
    }

    #[test]
    fn is_known_model_false_for_unknown() {
        assert!(!dialect::is_known_model("gemini-99"));
    }

    #[test]
    fn canonical_roundtrip() {
        let model = "gemini-2.5-pro";
        let canonical = dialect::to_canonical_model(model);
        let back = dialect::from_canonical_model(&canonical);
        assert_eq!(back, model);
    }

    // -- Capability manifest --

    #[test]
    fn capability_manifest_streaming_native() {
        let m = dialect::capability_manifest();
        assert_support_level!(m.get(&Capability::Streaming), Native);
    }

    #[test]
    fn capability_manifest_tool_glob_unsupported() {
        let m = dialect::capability_manifest();
        assert_support_level!(m.get(&Capability::ToolGlob), Unsupported);
    }

    #[test]
    fn capability_manifest_structured_output_native() {
        let m = dialect::capability_manifest();
        assert_support_level!(m.get(&Capability::StructuredOutputJsonSchema), Native);
    }

    // -- WorkOrder creation via From trait --

    #[test]
    fn gemini_request_to_work_order() {
        use abp_gemini_sdk::dialect::{GeminiContent, GeminiPart, GeminiRequest};

        let req = GeminiRequest {
            model: "gemini-2.5-flash".into(),
            contents: vec![GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::Text("Hello Gemini".into())],
            }],
            system_instruction: None,
            generation_config: None,
            safety_settings: None,
            tools: None,
            tool_config: None,
        };
        let wo: WorkOrder = req.into();
        assert_eq!(wo.task, "Hello Gemini");
    }

    #[test]
    fn gemini_request_with_system_instruction() {
        use abp_gemini_sdk::dialect::{GeminiContent, GeminiPart, GeminiRequest};

        let req = GeminiRequest {
            model: "gemini-2.5-flash".into(),
            contents: vec![GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::Text("hi".into())],
            }],
            system_instruction: Some(GeminiContent {
                role: "".into(),
                parts: vec![GeminiPart::Text("Be helpful".into())],
            }),
            generation_config: None,
            safety_settings: None,
            tools: None,
            tool_config: None,
        };
        let wo: WorkOrder = req.into();
        assert!(!wo.context.snippets.is_empty());
    }

    #[test]
    fn gemini_request_sets_model() {
        use abp_gemini_sdk::dialect::{GeminiContent, GeminiPart, GeminiRequest};

        let req = GeminiRequest {
            model: "gemini-2.5-pro".into(),
            contents: vec![GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::Text("hi".into())],
            }],
            system_instruction: None,
            generation_config: None,
            safety_settings: None,
            tools: None,
            tool_config: None,
        };
        let wo: WorkOrder = req.into();
        assert_eq!(wo.config.model.as_deref(), Some("gemini-2.5-pro"));
    }

    // -- Receipt to Response --

    #[test]
    fn receipt_to_gemini_response_maps_text() {
        use abp_gemini_sdk::dialect::GeminiResponse;

        let receipt = make_receipt_with_text("Hello from Gemini");
        let resp: GeminiResponse = receipt.into();
        assert!(!resp.candidates.is_empty());
        let has_text = resp.candidates[0].content.parts.iter().any(|p| {
            matches!(p, abp_gemini_sdk::dialect::GeminiPart::Text(t) if t == "Hello from Gemini")
        });
        assert!(has_text);
    }

    #[test]
    fn receipt_to_gemini_response_tool_calls() {
        use abp_gemini_sdk::dialect::GeminiResponse;

        let receipt = make_receipt_with_tool_call("search", "call_1", json!({"q": "rust"}));
        let resp: GeminiResponse = receipt.into();
        let has_func_call = resp.candidates[0].content.parts.iter().any(|p| {
            matches!(p, abp_gemini_sdk::dialect::GeminiPart::FunctionCall { name, .. } if name == "search")
        });
        assert!(has_func_call);
    }

    #[test]
    fn receipt_to_gemini_response_failed() {
        use abp_gemini_sdk::dialect::GeminiResponse;

        let receipt = make_receipt_failed();
        let resp: GeminiResponse = receipt.into();
        // Should still produce candidates
        assert!(!resp.candidates.is_empty());
    }

    // -- Tool definition translation --

    #[test]
    fn tool_def_to_gemini_roundtrip() {
        let canonical = dialect::CanonicalToolDef {
            name: "search".into(),
            description: "Web search".into(),
            parameters_schema: sample_tool_schema(),
        };
        let gemini = dialect::tool_def_to_gemini(&canonical);
        assert_eq!(gemini.name, "search");
        assert_eq!(gemini.parameters, canonical.parameters_schema);

        let back = dialect::tool_def_from_gemini(&gemini);
        assert_eq!(back.name, canonical.name);
        assert_eq!(back.description, canonical.description);
    }

    // -- Serde --

    #[test]
    fn part_text_serde() {
        let part = Part::Text("hello".into());
        let json = serde_json::to_value(&part).unwrap();
        assert_eq!(json["text"], "hello");
    }

    #[test]
    fn part_function_call_serde() {
        let part = Part::FunctionCall {
            name: "search".into(),
            args: json!({"query": "rust"}),
        };
        let json = serde_json::to_value(&part).unwrap();
        assert_eq!(json["functionCall"]["name"], "search");
    }

    #[test]
    fn content_serde_roundtrip() {
        let content = Content {
            role: Some("user".into()),
            parts: vec![Part::Text("hi".into())],
        };
        let serialized = serde_json::to_string(&content).unwrap();
        let back: Content = serde_json::from_str(&serialized).unwrap();
        assert_eq!(content, back);
    }

    #[test]
    fn dialect_version_constant() {
        assert_eq!(dialect::DIALECT_VERSION, "gemini/v0.1");
    }

    #[test]
    fn default_model_constant() {
        assert_eq!(dialect::DEFAULT_MODEL, "gemini-2.5-flash");
    }

    #[test]
    fn sidecar_script_path() {
        let root = std::path::Path::new("/fake");
        let script = abp_gemini_sdk::sidecar_script(root);
        assert_eq!(script, root.join("hosts/gemini/host.js"));
    }
}

// ============================================================================
// Codex SDK Tests
// ============================================================================
mod codex {
    use super::*;
    use abp_codex_sdk::api::*;
    use abp_codex_sdk::dialect;

    // -- Constants --

    #[test]
    fn backend_name_is_correct() {
        assert_eq!(abp_codex_sdk::BACKEND_NAME, "sidecar:codex");
    }

    #[test]
    fn host_script_relative_path() {
        assert_eq!(abp_codex_sdk::HOST_SCRIPT_RELATIVE, "hosts/codex/host.js");
    }

    // -- Model mapping --

    #[test]
    fn to_canonical_model_adds_prefix() {
        assert_eq!(
            dialect::to_canonical_model("codex-mini-latest"),
            "openai/codex-mini-latest"
        );
    }

    #[test]
    fn from_canonical_model_strips_prefix() {
        assert_eq!(
            dialect::from_canonical_model("openai/codex-mini-latest"),
            "codex-mini-latest"
        );
    }

    #[test]
    fn from_canonical_model_passthrough() {
        assert_eq!(dialect::from_canonical_model("other/model"), "other/model");
    }

    #[test]
    fn is_known_model_codex() {
        assert!(dialect::is_known_model("codex-mini-latest"));
    }

    #[test]
    fn is_known_model_false_for_unknown() {
        assert!(!dialect::is_known_model("codex-99"));
    }

    #[test]
    fn canonical_roundtrip() {
        let model = "codex-mini-latest";
        let canonical = dialect::to_canonical_model(model);
        let back = dialect::from_canonical_model(&canonical);
        assert_eq!(back, model);
    }

    // -- Capability manifest --

    #[test]
    fn capability_manifest_streaming_native() {
        let m = dialect::capability_manifest();
        assert_support_level!(m.get(&Capability::Streaming), Native);
    }

    #[test]
    fn capability_manifest_tool_read_native() {
        let m = dialect::capability_manifest();
        assert_support_level!(m.get(&Capability::ToolRead), Native);
    }

    #[test]
    fn capability_manifest_mcp_unsupported() {
        let m = dialect::capability_manifest();
        assert_support_level!(m.get(&Capability::McpClient), Unsupported);
    }

    // -- WorkOrder creation --

    #[test]
    fn session_request_to_work_order_extracts_task() {
        let req = CodexSessionRequest {
            model: "codex-mini-latest".into(),
            instructions: None,
            input: vec![CodexInputItem::Message {
                role: "user".into(),
                content: "Fix the bug".into(),
            }],
            tools: None,
            stream: None,
            previous_response_id: None,
            max_output_tokens: None,
            temperature: None,
        };
        let wo: WorkOrder = req.into();
        assert_eq!(wo.task, "Fix the bug");
    }

    #[test]
    fn session_request_with_instructions() {
        let req = CodexSessionRequest {
            model: "codex-mini-latest".into(),
            instructions: Some("Always write tests".into()),
            input: vec![CodexInputItem::Message {
                role: "user".into(),
                content: "fix".into(),
            }],
            tools: None,
            stream: None,
            previous_response_id: None,
            max_output_tokens: None,
            temperature: None,
        };
        let wo: WorkOrder = req.into();
        let has_instructions = wo
            .context
            .snippets
            .iter()
            .any(|s| s.name == "instructions" && s.content == "Always write tests");
        assert!(has_instructions);
    }

    #[test]
    fn session_request_sets_model() {
        let req = CodexSessionRequest {
            model: "gpt-4.1".into(),
            instructions: None,
            input: vec![CodexInputItem::Message {
                role: "user".into(),
                content: "hi".into(),
            }],
            tools: None,
            stream: None,
            previous_response_id: None,
            max_output_tokens: None,
            temperature: None,
        };
        let wo: WorkOrder = req.into();
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4.1"));
    }

    #[test]
    fn session_request_with_previous_response_id() {
        let req = CodexSessionRequest {
            model: "codex-mini-latest".into(),
            instructions: None,
            input: vec![CodexInputItem::Message {
                role: "user".into(),
                content: "continue".into(),
            }],
            tools: None,
            stream: None,
            previous_response_id: Some("prev_123".into()),
            max_output_tokens: None,
            temperature: None,
        };
        let wo: WorkOrder = req.into();
        assert!(wo.config.vendor.get("previous_response_id").is_some());
    }

    // -- Receipt to Response --

    #[test]
    fn receipt_to_session_response_maps_text() {
        let receipt = make_receipt_with_text("Fixed the bug");
        let resp: CodexSessionResponse = receipt.into();
        assert_eq!(resp.status, "completed");
        let has_text = resp.output.iter().any(|item| {
            matches!(item, CodexOutputItem::Message { content, .. } if content.iter().any(|c| matches!(c, CodexContentPart::OutputText { text } if text == "Fixed the bug")))
        });
        assert!(has_text);
    }

    #[test]
    fn receipt_to_session_response_tool_call() {
        let receipt = make_receipt_with_tool_call("edit_file", "fc_01", json!({"path": "lib.rs"}));
        let resp: CodexSessionResponse = receipt.into();
        let has_func_call = resp.output.iter().any(|item| {
            matches!(item, CodexOutputItem::FunctionCall { name, .. } if name == "edit_file")
        });
        assert!(has_func_call);
    }

    #[test]
    fn receipt_to_session_response_failed() {
        let receipt = make_receipt_failed();
        let resp: CodexSessionResponse = receipt.into();
        assert_eq!(resp.status, "failed");
    }

    #[test]
    fn receipt_to_session_response_partial() {
        let receipt = make_receipt_partial();
        let resp: CodexSessionResponse = receipt.into();
        assert_eq!(resp.status, "incomplete");
    }

    #[test]
    fn receipt_to_session_response_object_type() {
        let receipt = make_receipt_with_text("test");
        let resp: CodexSessionResponse = receipt.into();
        assert_eq!(resp.object, "response");
    }

    #[test]
    fn receipt_to_session_response_usage() {
        let receipt = make_receipt_with_usage(100, 50);
        let resp: CodexSessionResponse = receipt.into();
        let usage = resp.usage.unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.total_tokens, 150);
    }

    // -- Tool definition translation --

    #[test]
    fn tool_def_to_codex_roundtrip() {
        let canonical = dialect::CanonicalToolDef {
            name: "bash".into(),
            description: "Execute a shell command".into(),
            parameters_schema: sample_tool_schema(),
        };
        let codex = dialect::tool_def_to_codex(&canonical);
        assert_eq!(codex.function.name, "bash");
        assert_eq!(codex.tool_type, "function");

        let back = dialect::tool_def_from_codex(&codex);
        assert_eq!(back.name, canonical.name);
    }

    // -- Serde --

    #[test]
    fn codex_input_message_serde() {
        let item = CodexInputItem::Message {
            role: "user".into(),
            content: "hi".into(),
        };
        let json = serde_json::to_value(&item).unwrap();
        assert_eq!(json["type"], "message");
        assert_eq!(json["role"], "user");
    }

    #[test]
    fn codex_input_function_call_output_serde() {
        let item = CodexInputItem::FunctionCallOutput {
            call_id: "fc_01".into(),
            output: "result".into(),
        };
        let json = serde_json::to_value(&item).unwrap();
        assert_eq!(json["type"], "function_call_output");
    }

    #[test]
    fn codex_output_message_serde() {
        let item = CodexOutputItem::Message {
            role: "assistant".into(),
            content: vec![CodexContentPart::OutputText {
                text: "hello".into(),
            }],
        };
        let json = serde_json::to_value(&item).unwrap();
        assert_eq!(json["type"], "message");
    }

    #[test]
    fn codex_output_function_call_serde() {
        let item = CodexOutputItem::FunctionCall {
            id: "fc_01".into(),
            call_id: None,
            name: "search".into(),
            arguments: "{}".into(),
        };
        let json = serde_json::to_value(&item).unwrap();
        assert_eq!(json["type"], "function_call");
        assert_eq!(json["name"], "search");
    }

    #[test]
    fn dialect_version_constant() {
        assert_eq!(dialect::DIALECT_VERSION, "codex/v0.1");
    }

    #[test]
    fn default_model_constant() {
        assert_eq!(dialect::DEFAULT_MODEL, "codex-mini-latest");
    }

    #[test]
    fn sidecar_script_path() {
        let root = std::path::Path::new("/fake");
        let script = abp_codex_sdk::sidecar_script(root);
        assert_eq!(script, root.join("hosts/codex/host.js"));
    }
}

// ============================================================================
// Copilot SDK Tests
// ============================================================================
mod copilot {
    use super::*;
    use abp_copilot_sdk::api::*;
    use abp_copilot_sdk::dialect;

    // -- Constants --

    #[test]
    fn backend_name_is_correct() {
        assert_eq!(abp_copilot_sdk::BACKEND_NAME, "sidecar:copilot");
    }

    #[test]
    fn host_script_relative_path() {
        assert_eq!(
            abp_copilot_sdk::HOST_SCRIPT_RELATIVE,
            "hosts/copilot/host.js"
        );
    }

    // -- Model mapping --

    #[test]
    fn to_canonical_model_adds_prefix() {
        assert_eq!(dialect::to_canonical_model("gpt-4o"), "copilot/gpt-4o");
    }

    #[test]
    fn from_canonical_model_strips_prefix() {
        assert_eq!(dialect::from_canonical_model("copilot/gpt-4o"), "gpt-4o");
    }

    #[test]
    fn from_canonical_model_passthrough() {
        assert_eq!(
            dialect::from_canonical_model("openai/gpt-4o"),
            "openai/gpt-4o"
        );
    }

    #[test]
    fn is_known_model_gpt4o() {
        assert!(dialect::is_known_model("gpt-4o"));
    }

    #[test]
    fn is_known_model_claude_sonnet() {
        assert!(dialect::is_known_model("claude-sonnet-4"));
    }

    #[test]
    fn is_known_model_false_for_unknown() {
        assert!(!dialect::is_known_model("unknown-model"));
    }

    #[test]
    fn canonical_roundtrip() {
        let model = "gpt-4o-mini";
        let canonical = dialect::to_canonical_model(model);
        let back = dialect::from_canonical_model(&canonical);
        assert_eq!(back, model);
    }

    // -- Capability manifest --

    #[test]
    fn capability_manifest_streaming_native() {
        let m = dialect::capability_manifest();
        assert_support_level!(m.get(&Capability::Streaming), Native);
    }

    #[test]
    fn capability_manifest_web_search_native() {
        let m = dialect::capability_manifest();
        assert_support_level!(m.get(&Capability::ToolWebSearch), Native);
    }

    #[test]
    fn capability_manifest_tool_glob_unsupported() {
        let m = dialect::capability_manifest();
        assert_support_level!(m.get(&Capability::ToolGlob), Unsupported);
    }

    #[test]
    fn capability_manifest_mcp_unsupported() {
        let m = dialect::capability_manifest();
        assert_support_level!(m.get(&Capability::McpClient), Unsupported);
    }

    // -- WorkOrder creation --

    #[test]
    fn copilot_request_to_work_order_extracts_task() {
        let req = CopilotRequest {
            model: "gpt-4o".into(),
            messages: vec![CopilotMessage {
                role: "user".into(),
                content: "Explain this code".into(),
                name: None,
                function_call: None,
                copilot_references: vec![],
            }],
            stream: None,
            temperature: None,
            max_tokens: None,
            references: vec![],
            copilot_metadata: None,
        };
        let wo: WorkOrder = req.into();
        assert_eq!(wo.task, "Explain this code");
    }

    #[test]
    fn copilot_request_with_system_message() {
        let req = CopilotRequest {
            model: "gpt-4o".into(),
            messages: vec![
                CopilotMessage {
                    role: "system".into(),
                    content: "You are a code assistant".into(),
                    name: None,
                    function_call: None,
                    copilot_references: vec![],
                },
                CopilotMessage {
                    role: "user".into(),
                    content: "help".into(),
                    name: None,
                    function_call: None,
                    copilot_references: vec![],
                },
            ],
            stream: None,
            temperature: None,
            max_tokens: None,
            references: vec![],
            copilot_metadata: None,
        };
        let wo: WorkOrder = req.into();
        assert!(!wo.context.snippets.is_empty());
    }

    #[test]
    fn copilot_request_with_file_references() {
        let req = CopilotRequest {
            model: "gpt-4o".into(),
            messages: vec![CopilotMessage {
                role: "user".into(),
                content: "explain".into(),
                name: None,
                function_call: None,
                copilot_references: vec![],
            }],
            stream: None,
            temperature: None,
            max_tokens: None,
            references: vec![dialect::CopilotReference {
                ref_type: dialect::CopilotReferenceType::File,
                id: "file_1".into(),
                data: json!({"path": "src/main.rs"}),
                metadata: None,
            }],
            copilot_metadata: None,
        };
        let wo: WorkOrder = req.into();
        assert!(!wo.context.files.is_empty());
    }

    #[test]
    fn copilot_request_sets_model() {
        let req = CopilotRequest {
            model: "gpt-4-turbo".into(),
            messages: vec![CopilotMessage {
                role: "user".into(),
                content: "hi".into(),
                name: None,
                function_call: None,
                copilot_references: vec![],
            }],
            stream: None,
            temperature: None,
            max_tokens: None,
            references: vec![],
            copilot_metadata: None,
        };
        let wo: WorkOrder = req.into();
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4-turbo"));
    }

    // -- Receipt to Response --

    #[test]
    fn receipt_to_response_maps_text() {
        let receipt = make_receipt_with_text("Here is the explanation");
        let resp: CopilotResponse = receipt.into();
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Here is the explanation")
        );
    }

    #[test]
    fn receipt_to_response_function_call() {
        let receipt = make_receipt_with_tool_call("search", "fc_01", json!({"q": "rust"}));
        let resp: CopilotResponse = receipt.into();
        assert_eq!(
            resp.choices[0].finish_reason,
            CopilotFinishReason::FunctionCall
        );
        let fc = resp.choices[0].message.function_call.as_ref().unwrap();
        assert_eq!(fc.name, "search");
    }

    #[test]
    fn receipt_to_response_stop_finish_reason() {
        let receipt = make_receipt_with_text("done");
        let resp: CopilotResponse = receipt.into();
        assert_eq!(resp.choices[0].finish_reason, CopilotFinishReason::Stop);
    }

    #[test]
    fn receipt_to_response_failed() {
        let receipt = make_receipt_failed();
        let resp: CopilotResponse = receipt.into();
        assert_eq!(resp.choices[0].finish_reason, CopilotFinishReason::Stop);
    }

    #[test]
    fn receipt_to_response_object_type() {
        let receipt = make_receipt_with_text("test");
        let resp: CopilotResponse = receipt.into();
        assert_eq!(resp.object, "chat.completion");
    }

    // -- Reference types --

    #[test]
    fn reference_type_file_serde() {
        let json = serde_json::to_value(&dialect::CopilotReferenceType::File).unwrap();
        assert_eq!(json, "file");
    }

    #[test]
    fn reference_type_snippet_serde() {
        let json = serde_json::to_value(&dialect::CopilotReferenceType::Snippet).unwrap();
        assert_eq!(json, "snippet");
    }

    #[test]
    fn reference_type_repository_serde() {
        let json = serde_json::to_value(&dialect::CopilotReferenceType::Repository).unwrap();
        assert_eq!(json, "repository");
    }

    #[test]
    fn reference_type_web_search_result_serde() {
        let json = serde_json::to_value(&dialect::CopilotReferenceType::WebSearchResult).unwrap();
        assert_eq!(json, "web_search_result");
    }

    // -- Serde --

    #[test]
    fn finish_reason_serde_roundtrip() {
        let json = serde_json::to_value(&CopilotFinishReason::FunctionCall).unwrap();
        assert_eq!(json, "function_call");
        let back: CopilotFinishReason = serde_json::from_value(json).unwrap();
        assert_eq!(back, CopilotFinishReason::FunctionCall);
    }

    #[test]
    fn finish_reason_stop_serde() {
        let json = serde_json::to_value(&CopilotFinishReason::Stop).unwrap();
        assert_eq!(json, "stop");
    }

    #[test]
    fn dialect_version_constant() {
        assert_eq!(dialect::DIALECT_VERSION, "copilot/v0.1");
    }

    #[test]
    fn default_model_constant() {
        assert_eq!(dialect::DEFAULT_MODEL, "gpt-4o");
    }

    #[test]
    fn sidecar_script_path() {
        let root = std::path::Path::new("/fake");
        let script = abp_copilot_sdk::sidecar_script(root);
        assert_eq!(script, root.join("hosts/copilot/host.js"));
    }
}

// ============================================================================
// Kimi SDK Tests
// ============================================================================
mod kimi {
    use super::*;
    use abp_kimi_sdk::api::*;
    use abp_kimi_sdk::dialect;

    // -- Constants --

    #[test]
    fn backend_name_is_correct() {
        assert_eq!(abp_kimi_sdk::BACKEND_NAME, "sidecar:kimi");
    }

    #[test]
    fn host_script_relative_path() {
        assert_eq!(abp_kimi_sdk::HOST_SCRIPT_RELATIVE, "hosts/kimi/host.js");
    }

    // -- Model mapping --

    #[test]
    fn to_canonical_model_adds_prefix() {
        assert_eq!(
            dialect::to_canonical_model("moonshot-v1-8k"),
            "moonshot/moonshot-v1-8k"
        );
    }

    #[test]
    fn from_canonical_model_strips_prefix() {
        assert_eq!(
            dialect::from_canonical_model("moonshot/moonshot-v1-8k"),
            "moonshot-v1-8k"
        );
    }

    #[test]
    fn from_canonical_model_passthrough() {
        assert_eq!(
            dialect::from_canonical_model("openai/gpt-4o"),
            "openai/gpt-4o"
        );
    }

    #[test]
    fn is_known_model_moonshot() {
        assert!(dialect::is_known_model("moonshot-v1-8k"));
    }

    #[test]
    fn is_known_model_kimi_latest() {
        assert!(dialect::is_known_model("kimi-latest"));
    }

    #[test]
    fn is_known_model_k1() {
        assert!(dialect::is_known_model("k1"));
    }

    #[test]
    fn is_known_model_false_for_unknown() {
        assert!(!dialect::is_known_model("kimi-99"));
    }

    #[test]
    fn canonical_roundtrip() {
        let model = "moonshot-v1-128k";
        let canonical = dialect::to_canonical_model(model);
        let back = dialect::from_canonical_model(&canonical);
        assert_eq!(back, model);
    }

    // -- Capability manifest --

    #[test]
    fn capability_manifest_streaming_native() {
        let m = dialect::capability_manifest();
        assert_support_level!(m.get(&Capability::Streaming), Native);
    }

    #[test]
    fn capability_manifest_web_search_native() {
        let m = dialect::capability_manifest();
        assert_support_level!(m.get(&Capability::ToolWebSearch), Native);
    }

    #[test]
    fn capability_manifest_tool_edit_unsupported() {
        let m = dialect::capability_manifest();
        assert_support_level!(m.get(&Capability::ToolEdit), Unsupported);
    }

    #[test]
    fn capability_manifest_mcp_unsupported() {
        let m = dialect::capability_manifest();
        assert_support_level!(m.get(&Capability::McpClient), Unsupported);
    }

    // -- WorkOrder creation --

    #[test]
    fn kimi_request_to_work_order_extracts_task() {
        let req = KimiChatRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![KimiMessage::User {
                content: "Hello Kimi".into(),
            }],
            temperature: None,
            top_p: None,
            max_tokens: None,
            tools: None,
            tool_choice: None,
            stream: None,
            n: None,
            stop: None,
            presence_penalty: None,
            frequency_penalty: None,
        };
        let wo: WorkOrder = req.into();
        assert_eq!(wo.task, "Hello Kimi");
    }

    #[test]
    fn kimi_request_with_system_message() {
        let req = KimiChatRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![
                KimiMessage::System {
                    content: "You speak Chinese".into(),
                },
                KimiMessage::User {
                    content: "hi".into(),
                },
            ],
            temperature: None,
            top_p: None,
            max_tokens: None,
            tools: None,
            tool_choice: None,
            stream: None,
            n: None,
            stop: None,
            presence_penalty: None,
            frequency_penalty: None,
        };
        let wo: WorkOrder = req.into();
        assert!(!wo.context.snippets.is_empty());
    }

    #[test]
    fn kimi_request_sets_model() {
        let req = KimiChatRequest {
            model: "moonshot-v1-128k".into(),
            messages: vec![KimiMessage::User {
                content: "hi".into(),
            }],
            temperature: None,
            top_p: None,
            max_tokens: None,
            tools: None,
            tool_choice: None,
            stream: None,
            n: None,
            stop: None,
            presence_penalty: None,
            frequency_penalty: None,
        };
        let wo: WorkOrder = req.into();
        assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-128k"));
    }

    #[test]
    fn kimi_request_last_user_message_used() {
        let req = KimiChatRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![
                KimiMessage::User {
                    content: "first".into(),
                },
                KimiMessage::User {
                    content: "second".into(),
                },
            ],
            temperature: None,
            top_p: None,
            max_tokens: None,
            tools: None,
            tool_choice: None,
            stream: None,
            n: None,
            stop: None,
            presence_penalty: None,
            frequency_penalty: None,
        };
        let wo: WorkOrder = req.into();
        assert_eq!(wo.task, "second");
    }

    #[test]
    fn kimi_request_no_user_yields_empty_task() {
        let req = KimiChatRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![KimiMessage::System {
                content: "system only".into(),
            }],
            temperature: None,
            top_p: None,
            max_tokens: None,
            tools: None,
            tool_choice: None,
            stream: None,
            n: None,
            stop: None,
            presence_penalty: None,
            frequency_penalty: None,
        };
        let wo: WorkOrder = req.into();
        assert_eq!(wo.task, "");
    }

    // -- Receipt to Response --

    #[test]
    fn receipt_to_response_maps_text() {
        let receipt = make_receipt_with_text("你好");
        let resp: KimiChatResponse = receipt.into();
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(resp.choices[0].message.content.as_deref(), Some("你好"));
    }

    #[test]
    fn receipt_to_response_stop_finish_reason() {
        let receipt = make_receipt_with_text("done");
        let resp: KimiChatResponse = receipt.into();
        assert_eq!(resp.choices[0].finish_reason, KimiFinishReason::Stop);
    }

    #[test]
    fn receipt_to_response_tool_calls_finish_reason() {
        let receipt = make_receipt_with_tool_call("search", "call_0", json!({"q": "test"}));
        let resp: KimiChatResponse = receipt.into();
        assert_eq!(resp.choices[0].finish_reason, KimiFinishReason::ToolCalls);
        let tc = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tc[0].function.name, "search");
    }

    #[test]
    fn receipt_to_response_failed() {
        let receipt = make_receipt_failed();
        let resp: KimiChatResponse = receipt.into();
        assert_eq!(resp.choices[0].finish_reason, KimiFinishReason::Stop);
    }

    #[test]
    fn receipt_to_response_object_type() {
        let receipt = make_receipt_with_text("test");
        let resp: KimiChatResponse = receipt.into();
        assert_eq!(resp.object, "chat.completion");
    }

    #[test]
    fn receipt_to_response_empty_trace() {
        let receipt = make_receipt_empty();
        let resp: KimiChatResponse = receipt.into();
        assert!(resp.choices[0].message.content.is_none());
    }

    // -- Tool definition translation --

    #[test]
    fn tool_def_to_kimi_roundtrip() {
        let canonical = dialect::CanonicalToolDef {
            name: "web_search".into(),
            description: "Search the web".into(),
            parameters_schema: sample_tool_schema(),
        };
        let kimi = dialect::tool_def_to_kimi(&canonical);
        assert_eq!(kimi.function.name, "web_search");
        assert_eq!(kimi.tool_type, "function");

        let back = dialect::tool_def_from_kimi(&kimi);
        assert_eq!(back.name, canonical.name);
        assert_eq!(back.description, canonical.description);
    }

    // -- Serde --

    #[test]
    fn kimi_message_user_serde() {
        let msg = KimiMessage::User {
            content: "hi".into(),
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["role"], "user");
        let back: KimiMessage = serde_json::from_value(json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn kimi_message_system_serde() {
        let msg = KimiMessage::System {
            content: "instructions".into(),
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["role"], "system");
    }

    #[test]
    fn kimi_message_tool_serde() {
        let msg = KimiMessage::Tool {
            tool_call_id: "tc_1".into(),
            content: "result".into(),
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["role"], "tool");
    }

    #[test]
    fn kimi_finish_reason_serde() {
        let json = serde_json::to_value(&KimiFinishReason::ToolCalls).unwrap();
        assert_eq!(json, "tool_calls");
    }

    #[test]
    fn dialect_version_constant() {
        assert_eq!(dialect::DIALECT_VERSION, "kimi/v0.1");
    }

    #[test]
    fn default_model_constant() {
        assert_eq!(dialect::DEFAULT_MODEL, "moonshot-v1-8k");
    }

    #[test]
    fn sidecar_script_path() {
        let root = std::path::Path::new("/fake");
        let script = abp_kimi_sdk::sidecar_script(root);
        assert_eq!(script, root.join("hosts/kimi/host.js"));
    }
}

// ============================================================================
// Cross-SDK Tests
// ============================================================================
mod cross_sdk {
    use super::*;

    // -- Error code snake_case --

    #[test]
    fn error_code_as_str_is_snake_case() {
        assert_eq!(abp_error::ErrorCode::Internal.as_str(), "internal");
    }

    #[test]
    fn error_code_backend_not_found_snake_case() {
        assert_eq!(
            abp_error::ErrorCode::BackendNotFound.as_str(),
            "backend_not_found"
        );
    }

    #[test]
    fn error_code_backend_timeout_snake_case() {
        assert_eq!(
            abp_error::ErrorCode::BackendTimeout.as_str(),
            "backend_timeout"
        );
    }

    #[test]
    fn error_code_backend_unavailable_snake_case() {
        assert_eq!(
            abp_error::ErrorCode::BackendUnavailable.as_str(),
            "backend_unavailable"
        );
    }

    #[test]
    fn error_code_protocol_invalid_envelope_snake_case() {
        assert_eq!(
            abp_error::ErrorCode::ProtocolInvalidEnvelope.as_str(),
            "protocol_invalid_envelope"
        );
    }

    #[test]
    fn error_code_serde_snake_case() {
        let code = abp_error::ErrorCode::BackendNotFound;
        let json = serde_json::to_value(&code).unwrap();
        assert_eq!(json, "backend_not_found");
    }

    #[test]
    fn error_code_serde_roundtrip() {
        let code = abp_error::ErrorCode::BackendTimeout;
        let json = serde_json::to_value(&code).unwrap();
        let back: abp_error::ErrorCode = serde_json::from_value(json).unwrap();
        assert_eq!(back, code);
    }

    #[test]
    fn error_code_internal_serde() {
        let code = abp_error::ErrorCode::Internal;
        let json = serde_json::to_value(&code).unwrap();
        assert_eq!(json, "internal");
    }

    #[test]
    fn error_code_is_retryable() {
        assert!(abp_error::ErrorCode::BackendTimeout.is_retryable());
        assert!(abp_error::ErrorCode::BackendUnavailable.is_retryable());
        assert!(!abp_error::ErrorCode::Internal.is_retryable());
    }

    #[test]
    fn error_code_message_not_empty() {
        let code = abp_error::ErrorCode::BackendNotFound;
        assert!(!code.message().is_empty());
    }

    // -- All SDK backend names are unique --

    #[test]
    fn all_backend_names_unique() {
        let names = [
            abp_openai_sdk::BACKEND_NAME,
            abp_claude_sdk::BACKEND_NAME,
            abp_gemini_sdk::BACKEND_NAME,
            abp_codex_sdk::BACKEND_NAME,
            abp_copilot_sdk::BACKEND_NAME,
            abp_kimi_sdk::BACKEND_NAME,
        ];
        let set: std::collections::HashSet<_> = names.iter().collect();
        assert_eq!(set.len(), names.len(), "Backend names must be unique");
    }

    // -- All SDKs have valid dialect versions --

    #[test]
    fn all_dialect_versions_valid() {
        let versions = [
            abp_openai_sdk::dialect::DIALECT_VERSION,
            abp_claude_sdk::dialect::DIALECT_VERSION,
            abp_gemini_sdk::dialect::DIALECT_VERSION,
            abp_codex_sdk::dialect::DIALECT_VERSION,
            abp_copilot_sdk::dialect::DIALECT_VERSION,
            abp_kimi_sdk::dialect::DIALECT_VERSION,
        ];
        for v in &versions {
            assert!(
                v.contains("/v0.1"),
                "dialect version {} must contain /v0.1",
                v
            );
        }
    }

    // -- All SDKs support streaming natively --

    #[test]
    fn all_sdks_support_streaming() {
        let manifests = [
            abp_openai_sdk::dialect::capability_manifest(),
            abp_claude_sdk::dialect::capability_manifest(),
            abp_gemini_sdk::dialect::capability_manifest(),
            abp_codex_sdk::dialect::capability_manifest(),
            abp_copilot_sdk::dialect::capability_manifest(),
            abp_kimi_sdk::dialect::capability_manifest(),
        ];
        for m in &manifests {
            assert!(
                matches!(m.get(&Capability::Streaming), Some(SupportLevel::Native)),
                "All SDKs must natively support streaming"
            );
        }
    }

    // -- All SDKs have MCP unsupported or native --

    #[test]
    fn mcp_server_unsupported_across_all() {
        let manifests = [
            abp_openai_sdk::dialect::capability_manifest(),
            abp_gemini_sdk::dialect::capability_manifest(),
            abp_codex_sdk::dialect::capability_manifest(),
            abp_copilot_sdk::dialect::capability_manifest(),
            abp_kimi_sdk::dialect::capability_manifest(),
        ];
        for m in &manifests {
            assert_support_level!(m.get(&Capability::McpServer), Unsupported);
        }
    }

    // -- Canonical model name round trips for all SDKs --

    #[test]
    fn openai_model_roundtrip() {
        let m = "gpt-4o";
        let c = abp_openai_sdk::dialect::to_canonical_model(m);
        assert!(c.starts_with("openai/"));
        let back = abp_openai_sdk::dialect::from_canonical_model(&c);
        assert_eq!(back, m);
    }

    #[test]
    fn claude_model_roundtrip() {
        let m = "claude-sonnet-4-20250514";
        let c = abp_claude_sdk::dialect::to_canonical_model(m);
        assert!(c.starts_with("anthropic/"));
        let back = abp_claude_sdk::dialect::from_canonical_model(&c);
        assert_eq!(back, m);
    }

    #[test]
    fn gemini_model_roundtrip() {
        let m = "gemini-2.5-flash";
        let c = abp_gemini_sdk::dialect::to_canonical_model(m);
        assert!(c.starts_with("google/"));
        let back = abp_gemini_sdk::dialect::from_canonical_model(&c);
        assert_eq!(back, m);
    }

    #[test]
    fn codex_model_roundtrip() {
        let m = "codex-mini-latest";
        let c = abp_codex_sdk::dialect::to_canonical_model(m);
        assert!(c.starts_with("openai/"));
        let back = abp_codex_sdk::dialect::from_canonical_model(&c);
        assert_eq!(back, m);
    }

    #[test]
    fn copilot_model_roundtrip() {
        let m = "gpt-4o";
        let c = abp_copilot_sdk::dialect::to_canonical_model(m);
        assert!(c.starts_with("copilot/"));
        let back = abp_copilot_sdk::dialect::from_canonical_model(&c);
        assert_eq!(back, m);
    }

    #[test]
    fn kimi_model_roundtrip() {
        let m = "moonshot-v1-8k";
        let c = abp_kimi_sdk::dialect::to_canonical_model(m);
        assert!(c.starts_with("moonshot/"));
        let back = abp_kimi_sdk::dialect::from_canonical_model(&c);
        assert_eq!(back, m);
    }

    // -- Receipt conversion preserves outcome mapping --

    #[test]
    fn all_sdks_handle_empty_receipt() {
        let receipt = make_receipt_empty();

        // OpenAI
        let r = receipt.clone();
        let resp: abp_openai_sdk::api::ChatCompletionResponse = r.into();
        assert_eq!(resp.choices.len(), 1);

        // Claude
        let r = receipt.clone();
        let resp: abp_claude_sdk::messages::MessagesResponse = r.into();
        assert_eq!(resp.role, "assistant");

        // Gemini
        let r = receipt.clone();
        let resp: abp_gemini_sdk::dialect::GeminiResponse = r.into();
        assert!(!resp.candidates.is_empty());

        // Codex
        let r = receipt.clone();
        let resp: abp_codex_sdk::api::CodexSessionResponse = r.into();
        assert_eq!(resp.status, "completed");

        // Copilot
        let r = receipt.clone();
        let resp: abp_copilot_sdk::api::CopilotResponse = r.into();
        assert_eq!(resp.choices.len(), 1);

        // Kimi
        let r = receipt.clone();
        let resp: abp_kimi_sdk::api::KimiChatResponse = r.into();
        assert_eq!(resp.choices.len(), 1);
    }

    #[test]
    fn all_sdks_handle_failed_receipt() {
        let receipt = make_receipt_failed();

        let r = receipt.clone();
        let resp: abp_openai_sdk::api::ChatCompletionResponse = r.into();
        assert_eq!(
            resp.choices[0].finish_reason,
            abp_openai_sdk::api::FinishReason::Stop
        );

        let r = receipt.clone();
        let resp: abp_codex_sdk::api::CodexSessionResponse = r.into();
        assert_eq!(resp.status, "failed");

        let r = receipt.clone();
        let resp: abp_copilot_sdk::api::CopilotResponse = r.into();
        assert_eq!(
            resp.choices[0].finish_reason,
            abp_copilot_sdk::api::CopilotFinishReason::Stop
        );

        let r = receipt.clone();
        let resp: abp_kimi_sdk::api::KimiChatResponse = r.into();
        assert_eq!(
            resp.choices[0].finish_reason,
            abp_kimi_sdk::api::KimiFinishReason::Stop
        );
    }

    // -- Passthrough mode --

    #[test]
    fn execution_mode_default_is_mapped() {
        let mode = ExecutionMode::default();
        assert_eq!(mode, ExecutionMode::Mapped);
    }

    #[test]
    fn execution_mode_passthrough_serde() {
        let json = serde_json::to_value(&ExecutionMode::Passthrough).unwrap();
        let back: ExecutionMode = serde_json::from_value(json).unwrap();
        assert_eq!(back, ExecutionMode::Passthrough);
    }

    #[test]
    fn execution_mode_mapped_serde() {
        let json = serde_json::to_value(&ExecutionMode::Mapped).unwrap();
        let back: ExecutionMode = serde_json::from_value(json).unwrap();
        assert_eq!(back, ExecutionMode::Mapped);
    }

    #[test]
    fn receipt_builder_mode_passthrough() {
        let receipt = ReceiptBuilder::new("mock")
            .mode(ExecutionMode::Passthrough)
            .outcome(Outcome::Complete)
            .build();
        assert_eq!(receipt.mode, ExecutionMode::Passthrough);
    }

    #[test]
    fn receipt_builder_mode_mapped() {
        let receipt = ReceiptBuilder::new("mock")
            .mode(ExecutionMode::Mapped)
            .outcome(Outcome::Complete)
            .build();
        assert_eq!(receipt.mode, ExecutionMode::Mapped);
    }

    // -- Capability support levels --

    #[test]
    fn support_level_native_serde() {
        let json = serde_json::to_value(&SupportLevel::Native).unwrap();
        let back: SupportLevel = serde_json::from_value(json).unwrap();
        assert!(matches!(back, SupportLevel::Native));
    }

    #[test]
    fn support_level_emulated_serde() {
        let json = serde_json::to_value(&SupportLevel::Emulated).unwrap();
        let back: SupportLevel = serde_json::from_value(json).unwrap();
        assert!(matches!(back, SupportLevel::Emulated));
    }

    #[test]
    fn support_level_unsupported_serde() {
        let json = serde_json::to_value(&SupportLevel::Unsupported).unwrap();
        let back: SupportLevel = serde_json::from_value(json).unwrap();
        assert!(matches!(back, SupportLevel::Unsupported));
    }

    // -- Multiple tool calls in a receipt --

    #[test]
    fn openai_receipt_with_multiple_tool_calls() {
        let receipt = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .add_trace_event(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::ToolCall {
                    tool_name: "read_file".into(),
                    tool_use_id: Some("call_0".into()),
                    parent_tool_use_id: None,
                    input: json!({"path": "a.txt"}),
                },
                ext: None,
            })
            .add_trace_event(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::ToolCall {
                    tool_name: "write_file".into(),
                    tool_use_id: Some("call_1".into()),
                    parent_tool_use_id: None,
                    input: json!({"path": "b.txt", "content": "hi"}),
                },
                ext: None,
            })
            .build();

        let resp: abp_openai_sdk::api::ChatCompletionResponse = receipt.into();
        let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tcs.len(), 2);
        assert_eq!(tcs[0].function.name, "read_file");
        assert_eq!(tcs[1].function.name, "write_file");
    }

    // -- Text concatenation from multiple events --

    #[test]
    fn kimi_receipt_concatenates_text_events() {
        let receipt = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .add_trace_event(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: "Hello ".into(),
                },
                ext: None,
            })
            .add_trace_event(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: "World".into(),
                },
                ext: None,
            })
            .build();

        let resp: abp_kimi_sdk::api::KimiChatResponse = receipt.into();
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Hello World")
        );
    }

    #[test]
    fn openai_receipt_concatenates_text_events() {
        let receipt = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .add_trace_event(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: "Hello ".into(),
                },
                ext: None,
            })
            .add_trace_event(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: "World".into(),
                },
                ext: None,
            })
            .build();

        let resp: abp_openai_sdk::api::ChatCompletionResponse = receipt.into();
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Hello World")
        );
    }

    // -- Contract version present in receipt --

    #[test]
    fn receipt_has_contract_version() {
        let receipt = make_receipt_with_text("test");
        assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    }
}
