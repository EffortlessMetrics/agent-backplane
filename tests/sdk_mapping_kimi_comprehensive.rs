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
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]
//! Comprehensive tests for Moonshot Kimi SDK dialect mapping.
//!
//! Validates the full round-trip from Kimi request/response formats
//! through ABP's intermediate representation and back, covering:
//!   1. Chat completion request mapping to WorkOrder
//!   2. Function calling / tool_use mapping
//!   3. SSE streaming response mapping
//!   4. Model name and capability mapping
//!   5. Kimi-specific features (search, built-in tools, citations)
//!   6. Token counting and context window handling
//!   7. Kimi→OpenAI compatibility (OpenAI-compatible API surface)
//!   8. Error type mapping
//!   9. Multi-modal content handling
//!  10. Conversation history format

use std::collections::BTreeMap;

use abp_core::ir::{IrContentBlock, IrRole, IrUsage};
use abp_core::{AgentEvent, AgentEventKind, CONTRACT_VERSION, UsageNormalized, WorkOrderBuilder};
use abp_dialect::Dialect;
use abp_kimi_sdk::dialect::{
    self, CanonicalToolDef, KimiBuiltinFunction, KimiBuiltinTool, KimiChoice, KimiChunk,
    KimiChunkChoice, KimiChunkDelta, KimiChunkFunctionCall, KimiChunkToolCall, KimiConfig,
    KimiFunctionCall, KimiFunctionDef, KimiMessage, KimiRef, KimiRequest, KimiResponse,
    KimiResponseMessage, KimiRole, KimiTool, KimiToolCall, KimiUsage, ToolCallAccumulator,
};
use abp_kimi_sdk::lowering;
use abp_mapping::{Fidelity, MappingError, features, known_rules, validate_mapping};
use abp_shim_kimi::{
    KimiClient, KimiRequestBuilder, Message, ProcessFn, events_to_stream_chunks, ir_to_messages,
    ir_usage_to_usage, messages_to_ir, mock_receipt, mock_receipt_with_usage, receipt_to_response,
    request_to_work_order, response_to_ir,
};
use chrono::Utc;
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn kimi_msg(role: &str, content: Option<&str>) -> KimiMessage {
    KimiMessage {
        role: role.into(),
        content: content.map(Into::into),
        tool_call_id: None,
        tool_calls: None,
    }
}

fn kimi_tool_result_msg(content: Option<&str>, tool_call_id: &str) -> KimiMessage {
    KimiMessage {
        role: "tool".into(),
        content: content.map(Into::into),
        tool_call_id: Some(tool_call_id.into()),
        tool_calls: None,
    }
}

fn assistant_with_tool_calls(content: Option<&str>, calls: Vec<KimiToolCall>) -> KimiMessage {
    KimiMessage {
        role: "assistant".into(),
        content: content.map(Into::into),
        tool_call_id: None,
        tool_calls: Some(calls),
    }
}

fn make_kimi_tool_call(id: &str, name: &str, args: &str) -> KimiToolCall {
    KimiToolCall {
        id: id.into(),
        call_type: "function".into(),
        function: KimiFunctionCall {
            name: name.into(),
            arguments: args.into(),
        },
    }
}

fn simple_kimi_response(text: &str) -> KimiResponse {
    KimiResponse {
        id: "cmpl-test".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChoice {
            index: 0,
            message: KimiResponseMessage {
                role: "assistant".into(),
                content: Some(text.into()),
                tool_calls: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: None,
        refs: None,
    }
}

fn make_chunk(id: &str, delta: KimiChunkDelta, finish_reason: Option<&str>) -> KimiChunk {
    KimiChunk {
        id: id.into(),
        object: "chat.completion.chunk".into(),
        created: 1_700_000_000,
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChunkChoice {
            index: 0,
            delta,
            finish_reason: finish_reason.map(Into::into),
        }],
        usage: None,
        refs: None,
    }
}

fn assistant_msg_event(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: text.to_string(),
        },
        ext: None,
    }
}

fn assistant_delta_event(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta {
            text: text.to_string(),
        },
        ext: None,
    }
}

fn tool_call_event(name: &str, id: &str, input: serde_json::Value) -> AgentEvent {
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

fn error_event(message: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: message.into(),
            error_code: None,
        },
        ext: None,
    }
}

fn make_processor(events: Vec<AgentEvent>) -> ProcessFn {
    Box::new(move |_wo| mock_receipt(events.clone()))
}

fn make_processor_with_usage(events: Vec<AgentEvent>, usage: UsageNormalized) -> ProcessFn {
    Box::new(move |_wo| mock_receipt_with_usage(events.clone(), usage.clone()))
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 1: Chat Completion Request Mapping
// ═══════════════════════════════════════════════════════════════════════════

mod chat_completion_request_mapping {
    use super::*;

    #[test]
    fn minimal_request_to_work_order() {
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("Hello")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.task, "Hello");
        assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-8k"));
    }

    #[test]
    fn request_preserves_model_name() {
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-128k")
            .messages(vec![Message::user("Hi")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-128k"));
    }

    #[test]
    fn request_with_temperature_in_vendor_config() {
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("Hi")])
            .temperature(0.7)
            .build();
        let wo = request_to_work_order(&req);
        let temp = wo.config.vendor.get("temperature");
        assert!(temp.is_some());
        assert_eq!(temp.unwrap().as_f64(), Some(0.7));
    }

    #[test]
    fn request_with_max_tokens_in_vendor_config() {
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("Hi")])
            .max_tokens(2048)
            .build();
        let wo = request_to_work_order(&req);
        let max_t = wo.config.vendor.get("max_tokens");
        assert!(max_t.is_some());
        assert_eq!(max_t.unwrap().as_u64(), Some(2048));
    }

    #[test]
    fn default_model_is_moonshot_v1_8k() {
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("Hi")])
            .build();
        assert_eq!(req.model, "moonshot-v1-8k");
    }

    #[test]
    fn request_serde_roundtrip_minimal() {
        let req = KimiRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![kimi_msg("user", Some("Hi"))],
            max_tokens: None,
            temperature: None,
            stream: None,
            tools: None,
            use_search: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: KimiRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.model, "moonshot-v1-8k");
        assert_eq!(back.messages.len(), 1);
    }

    #[test]
    fn request_serde_roundtrip_with_all_fields() {
        let req = KimiRequest {
            model: "moonshot-v1-32k".into(),
            messages: vec![
                kimi_msg("system", Some("Be helpful.")),
                kimi_msg("user", Some("Hello")),
            ],
            max_tokens: Some(4096),
            temperature: Some(0.5),
            stream: Some(true),
            tools: Some(vec![KimiTool::Function {
                function: KimiFunctionDef {
                    name: "search".into(),
                    description: "Search the web".into(),
                    parameters: json!({"type": "object"}),
                },
            }]),
            use_search: Some(true),
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: KimiRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.model, "moonshot-v1-32k");
        assert_eq!(back.max_tokens, Some(4096));
        assert_eq!(back.temperature, Some(0.5));
        assert_eq!(back.stream, Some(true));
        assert_eq!(back.use_search, Some(true));
        assert!(back.tools.is_some());
    }

    #[test]
    fn system_plus_user_request_extracts_user_task() {
        let req = KimiRequestBuilder::new()
            .messages(vec![
                Message::system("You are helpful."),
                Message::user("Explain async in Rust"),
            ])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.task, "Explain async in Rust");
    }

    #[test]
    fn multi_user_messages_extracts_last_user_as_task() {
        let req = KimiRequestBuilder::new()
            .messages(vec![
                Message::user("First message"),
                Message::assistant("Ack"),
                Message::user("Second message"),
            ])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.task, "Second message");
    }

    #[test]
    fn work_order_has_contract_version() {
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("test")])
            .build();
        let wo = request_to_work_order(&req);
        // WorkOrder always carries the contract version in the receipt produced
        assert_eq!(CONTRACT_VERSION, "abp/v0.1");
        assert!(!wo.task.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 2: Function Calling / Tool Use Mapping
// ═══════════════════════════════════════════════════════════════════════════

mod function_calling_tool_use {
    use super::*;

    #[test]
    fn canonical_tool_def_to_kimi_and_back() {
        let canonical = CanonicalToolDef {
            name: "web_search".into(),
            description: "Search the web".into(),
            parameters_schema: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"}
                }
            }),
        };
        let kimi = dialect::tool_def_to_kimi(&canonical);
        assert_eq!(kimi.tool_type, "function");
        assert_eq!(kimi.function.name, "web_search");

        let back = dialect::tool_def_from_kimi(&kimi);
        assert_eq!(back, canonical);
    }

    #[test]
    fn tool_call_in_response_maps_to_agent_event() {
        let resp = KimiResponse {
            id: "cmpl-abc".into(),
            model: "moonshot-v1-8k".into(),
            choices: vec![KimiChoice {
                index: 0,
                message: KimiResponseMessage {
                    role: "assistant".into(),
                    content: None,
                    tool_calls: Some(vec![make_kimi_tool_call(
                        "call_1",
                        "get_weather",
                        r#"{"city":"Beijing"}"#,
                    )]),
                },
                finish_reason: Some("tool_calls".into()),
            }],
            usage: None,
            refs: None,
        };
        let events = dialect::map_response(&resp);
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
                assert_eq!(input, &json!({"city": "Beijing"}));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn multiple_tool_calls_in_single_response() {
        let resp = KimiResponse {
            id: "cmpl-multi".into(),
            model: "moonshot-v1-8k".into(),
            choices: vec![KimiChoice {
                index: 0,
                message: KimiResponseMessage {
                    role: "assistant".into(),
                    content: None,
                    tool_calls: Some(vec![
                        make_kimi_tool_call("call_1", "search", r#"{"q":"rust"}"#),
                        make_kimi_tool_call("call_2", "fetch", r#"{"url":"https://example.com"}"#),
                    ]),
                },
                finish_reason: Some("tool_calls".into()),
            }],
            usage: None,
            refs: None,
        };
        let events = dialect::map_response(&resp);
        assert_eq!(events.len(), 2);
        assert!(
            matches!(&events[0].kind, AgentEventKind::ToolCall { tool_name, .. } if tool_name == "search")
        );
        assert!(
            matches!(&events[1].kind, AgentEventKind::ToolCall { tool_name, .. } if tool_name == "fetch")
        );
    }

    #[test]
    fn tool_call_with_text_content_produces_both_events() {
        let resp = KimiResponse {
            id: "cmpl-both".into(),
            model: "moonshot-v1-8k".into(),
            choices: vec![KimiChoice {
                index: 0,
                message: KimiResponseMessage {
                    role: "assistant".into(),
                    content: Some("Let me search for that.".into()),
                    tool_calls: Some(vec![make_kimi_tool_call(
                        "call_1",
                        "web_search",
                        r#"{"query":"rust"}"#,
                    )]),
                },
                finish_reason: Some("tool_calls".into()),
            }],
            usage: None,
            refs: None,
        };
        let events = dialect::map_response(&resp);
        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0].kind,
            AgentEventKind::AssistantMessage { .. }
        ));
        assert!(matches!(&events[1].kind, AgentEventKind::ToolCall { .. }));
    }

    #[test]
    fn malformed_tool_arguments_kept_as_string() {
        let resp = KimiResponse {
            id: "cmpl-bad".into(),
            model: "moonshot-v1-8k".into(),
            choices: vec![KimiChoice {
                index: 0,
                message: KimiResponseMessage {
                    role: "assistant".into(),
                    content: None,
                    tool_calls: Some(vec![make_kimi_tool_call("call_x", "foo", "not-json")]),
                },
                finish_reason: Some("tool_calls".into()),
            }],
            usage: None,
            refs: None,
        };
        let events = dialect::map_response(&resp);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::ToolCall { input, .. } => {
                assert_eq!(input, &serde_json::Value::String("not-json".into()));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn tool_result_message_ir_roundtrip() {
        let msgs = vec![
            kimi_msg("user", Some("search for rust")),
            assistant_with_tool_calls(
                None,
                vec![make_kimi_tool_call("c1", "search", r#"{"q":"rust"}"#)],
            ),
            kimi_tool_result_msg(Some("results here"), "c1"),
        ];
        let conv = lowering::to_ir(&msgs);
        assert_eq!(conv.len(), 3);
        assert_eq!(conv.messages[2].role, IrRole::Tool);

        let back = lowering::from_ir(&conv);
        assert_eq!(back[2].role, "tool");
        assert_eq!(back[2].tool_call_id.as_deref(), Some("c1"));
    }

    #[test]
    fn kimi_tool_enum_function_serde() {
        let tool = KimiTool::Function {
            function: KimiFunctionDef {
                name: "calc".into(),
                description: "Calculate".into(),
                parameters: json!({"type": "object"}),
            },
        };
        let json_str = serde_json::to_string(&tool).unwrap();
        assert!(json_str.contains("\"type\":\"function\""));
        let back: KimiTool = serde_json::from_str(&json_str).unwrap();
        assert!(matches!(back, KimiTool::Function { .. }));
    }

    #[test]
    fn kimi_tool_enum_builtin_serde() {
        let tool = KimiTool::BuiltinFunction {
            function: KimiBuiltinFunction {
                name: "$web_search".into(),
            },
        };
        let json_str = serde_json::to_string(&tool).unwrap();
        assert!(json_str.contains("\"type\":\"builtin_function\""));
        let back: KimiTool = serde_json::from_str(&json_str).unwrap();
        assert!(matches!(back, KimiTool::BuiltinFunction { .. }));
    }

    #[tokio::test]
    async fn client_tool_call_roundtrip() {
        let events = vec![tool_call_event(
            "web_search",
            "call_abc",
            json!({"query": "rust async"}),
        )];
        let client = KimiClient::new("moonshot-v1-8k").with_processor(make_processor(events));
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("Search for rust async")])
            .build();
        let resp = client.create(req).await.unwrap();
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
        let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tcs[0].function.name, "web_search");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 3: Streaming Response Mapping (SSE)
// ═══════════════════════════════════════════════════════════════════════════

mod streaming_response_mapping {
    use super::*;

    #[test]
    fn text_delta_chunk_maps_to_assistant_delta() {
        let chunk = make_chunk(
            "chunk-1",
            KimiChunkDelta {
                role: None,
                content: Some("Hello".into()),
                tool_calls: None,
            },
            None,
        );
        let events = dialect::map_stream_event(&chunk);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::AssistantDelta { text } => assert_eq!(text, "Hello"),
            other => panic!("expected AssistantDelta, got {other:?}"),
        }
    }

    #[test]
    fn empty_content_delta_produces_no_events() {
        let chunk = make_chunk(
            "chunk-2",
            KimiChunkDelta {
                role: Some("assistant".into()),
                content: Some(String::new()),
                tool_calls: None,
            },
            None,
        );
        let events = dialect::map_stream_event(&chunk);
        assert!(events.is_empty());
    }

    #[test]
    fn finish_reason_produces_run_completed() {
        let chunk = make_chunk("chunk-3", KimiChunkDelta::default(), Some("stop"));
        let events = dialect::map_stream_event(&chunk);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::RunCompleted { message } => {
                assert!(message.contains("stop"));
            }
            other => panic!("expected RunCompleted, got {other:?}"),
        }
    }

    #[test]
    fn tool_call_accumulator_single_tool() {
        let mut acc = ToolCallAccumulator::new();
        acc.feed(&[KimiChunkToolCall {
            index: 0,
            id: Some("call_1".into()),
            call_type: Some("function".into()),
            function: Some(KimiChunkFunctionCall {
                name: Some("search".into()),
                arguments: Some(r#"{"q":"#.into()),
            }),
        }]);
        acc.feed(&[KimiChunkToolCall {
            index: 0,
            id: None,
            call_type: None,
            function: Some(KimiChunkFunctionCall {
                name: None,
                arguments: Some(r#""rust"}"#.into()),
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
                assert_eq!(tool_name, "search");
                assert_eq!(tool_use_id.as_deref(), Some("call_1"));
                assert_eq!(input, &json!({"q": "rust"}));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn tool_call_accumulator_multiple_tools() {
        let mut acc = ToolCallAccumulator::new();
        acc.feed(&[
            KimiChunkToolCall {
                index: 0,
                id: Some("call_a".into()),
                call_type: Some("function".into()),
                function: Some(KimiChunkFunctionCall {
                    name: Some("alpha".into()),
                    arguments: Some("{}".into()),
                }),
            },
            KimiChunkToolCall {
                index: 1,
                id: Some("call_b".into()),
                call_type: Some("function".into()),
                function: Some(KimiChunkFunctionCall {
                    name: Some("beta".into()),
                    arguments: Some("{}".into()),
                }),
            },
        ]);
        let events = acc.finish();
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn tool_call_accumulator_empty_name_filtered() {
        let mut acc = ToolCallAccumulator::new();
        acc.feed(&[KimiChunkToolCall {
            index: 0,
            id: Some("call_x".into()),
            call_type: Some("function".into()),
            function: None,
        }]);
        let events = acc.finish();
        assert!(events.is_empty());
    }

    #[test]
    fn stream_events_to_chunks_contain_correct_model() {
        let events = vec![assistant_delta_event("Hello")];
        let chunks = events_to_stream_chunks(&events, "moonshot-v1-32k");
        for chunk in &chunks {
            assert_eq!(chunk.model, "moonshot-v1-32k");
            assert_eq!(chunk.object, "chat.completion.chunk");
        }
    }

    #[test]
    fn stream_chunks_end_with_stop() {
        let events = vec![assistant_delta_event("hi")];
        let chunks = events_to_stream_chunks(&events, "moonshot-v1-8k");
        // 1 delta + 1 stop
        assert_eq!(chunks.len(), 2);
        assert_eq!(
            chunks.last().unwrap().choices[0].finish_reason.as_deref(),
            Some("stop")
        );
    }

    #[test]
    fn chunk_with_refs_attaches_ext_metadata() {
        let chunk = KimiChunk {
            id: "chunk-ref".into(),
            object: "chat.completion.chunk".into(),
            created: 1_700_000_000,
            model: "moonshot-v1-8k".into(),
            choices: vec![KimiChunkChoice {
                index: 0,
                delta: KimiChunkDelta {
                    role: None,
                    content: Some("According to [1]...".into()),
                    tool_calls: None,
                },
                finish_reason: None,
            }],
            usage: None,
            refs: Some(vec![KimiRef {
                index: 1,
                url: "https://example.com".into(),
                title: Some("Example".into()),
            }]),
        };
        let events = dialect::map_stream_event(&chunk);
        assert_eq!(events.len(), 1);
        let ext = events[0].ext.as_ref().unwrap();
        assert!(ext.contains_key("kimi_refs"));
    }

    #[tokio::test]
    async fn client_streaming_produces_chunks() {
        let events = vec![assistant_delta_event("Hel"), assistant_delta_event("lo!")];
        let client = KimiClient::new("moonshot-v1-8k").with_processor(make_processor(events));
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("Hi")])
            .stream(true)
            .build();
        use tokio_stream::StreamExt;
        let stream = client.create_stream(req).await.unwrap();
        let chunks: Vec<KimiChunk> = stream.collect().await;
        // 2 deltas + 1 stop
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].choices[0].delta.content.as_deref(), Some("Hel"));
        assert_eq!(chunks[1].choices[0].delta.content.as_deref(), Some("lo!"));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 4: Model Name and Capability Mapping
// ═══════════════════════════════════════════════════════════════════════════

mod model_capability_mapping {
    use super::*;
    use abp_core::{Capability, SupportLevel};

    #[test]
    fn known_models_recognized() {
        assert!(dialect::is_known_model("moonshot-v1-8k"));
        assert!(dialect::is_known_model("moonshot-v1-32k"));
        assert!(dialect::is_known_model("moonshot-v1-128k"));
        assert!(dialect::is_known_model("kimi-latest"));
        assert!(dialect::is_known_model("k1"));
    }

    #[test]
    fn unknown_model_not_recognized() {
        assert!(!dialect::is_known_model("gpt-4o"));
        assert!(!dialect::is_known_model("claude-3-opus"));
        assert!(!dialect::is_known_model("moonshot-v2"));
    }

    #[test]
    fn canonical_model_roundtrip() {
        let canonical = dialect::to_canonical_model("moonshot-v1-8k");
        assert_eq!(canonical, "moonshot/moonshot-v1-8k");
        let back = dialect::from_canonical_model(&canonical);
        assert_eq!(back, "moonshot-v1-8k");
    }

    #[test]
    fn canonical_model_strips_prefix() {
        assert_eq!(
            dialect::from_canonical_model("moonshot/kimi-latest"),
            "kimi-latest"
        );
    }

    #[test]
    fn canonical_model_no_prefix_passthrough() {
        assert_eq!(dialect::from_canonical_model("gpt-4o"), "gpt-4o");
    }

    #[test]
    fn capability_manifest_has_streaming_native() {
        let caps = dialect::capability_manifest();
        assert!(matches!(
            caps.get(&Capability::Streaming),
            Some(SupportLevel::Native)
        ));
    }

    #[test]
    fn capability_manifest_has_tool_read_native() {
        let caps = dialect::capability_manifest();
        assert!(matches!(
            caps.get(&Capability::ToolRead),
            Some(SupportLevel::Native)
        ));
    }

    #[test]
    fn capability_manifest_has_web_search_native() {
        let caps = dialect::capability_manifest();
        assert!(matches!(
            caps.get(&Capability::ToolWebSearch),
            Some(SupportLevel::Native)
        ));
    }

    #[test]
    fn capability_manifest_tool_edit_unsupported() {
        let caps = dialect::capability_manifest();
        assert!(matches!(
            caps.get(&Capability::ToolEdit),
            Some(SupportLevel::Unsupported)
        ));
    }

    #[test]
    fn capability_manifest_tool_bash_unsupported() {
        let caps = dialect::capability_manifest();
        assert!(matches!(
            caps.get(&Capability::ToolBash),
            Some(SupportLevel::Unsupported)
        ));
    }

    #[test]
    fn capability_manifest_mcp_unsupported() {
        let caps = dialect::capability_manifest();
        assert!(matches!(
            caps.get(&Capability::McpClient),
            Some(SupportLevel::Unsupported)
        ));
        assert!(matches!(
            caps.get(&Capability::McpServer),
            Some(SupportLevel::Unsupported)
        ));
    }

    #[test]
    fn capability_manifest_structured_output_emulated() {
        let caps = dialect::capability_manifest();
        assert!(matches!(
            caps.get(&Capability::StructuredOutputJsonSchema),
            Some(SupportLevel::Emulated)
        ));
    }

    #[test]
    fn capability_manifest_is_deterministic_btreemap() {
        let caps1 = dialect::capability_manifest();
        let caps2 = dialect::capability_manifest();
        let json1 = serde_json::to_string(&caps1).unwrap();
        let json2 = serde_json::to_string(&caps2).unwrap();
        assert_eq!(json1, json2);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 5: Kimi-Specific Features (Search, Built-in Tools, Citations)
// ═══════════════════════════════════════════════════════════════════════════

mod kimi_specific_features {
    use super::*;

    #[test]
    fn builtin_search_internet_tool() {
        let tool = dialect::builtin_search_internet();
        assert_eq!(tool.tool_type, "builtin_function");
        assert_eq!(tool.function.name, "$web_search");
    }

    #[test]
    fn builtin_browser_tool() {
        let tool = dialect::builtin_browser();
        assert_eq!(tool.tool_type, "builtin_function");
        assert_eq!(tool.function.name, "$browser");
    }

    #[test]
    fn builtin_search_serde_roundtrip() {
        let tool = dialect::builtin_search_internet();
        let json = serde_json::to_string(&tool).unwrap();
        let back: KimiBuiltinTool = serde_json::from_str(&json).unwrap();
        assert_eq!(back, tool);
    }

    #[test]
    fn response_with_refs_maps_to_events_with_ext() {
        let resp = KimiResponse {
            id: "cmpl-ref".into(),
            model: "moonshot-v1-8k".into(),
            choices: vec![KimiChoice {
                index: 0,
                message: KimiResponseMessage {
                    role: "assistant".into(),
                    content: Some("According to [1], Rust is great.".into()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: None,
            refs: Some(vec![
                KimiRef {
                    index: 1,
                    url: "https://rust-lang.org".into(),
                    title: Some("Rust Programming Language".into()),
                },
                KimiRef {
                    index: 2,
                    url: "https://docs.rs".into(),
                    title: None,
                },
            ]),
        };
        let events = dialect::map_response(&resp);
        assert_eq!(events.len(), 1);
        let ext = events[0].ext.as_ref().expect("should have ext with refs");
        let refs_json = ext.get("kimi_refs").unwrap();
        let refs: Vec<KimiRef> = serde_json::from_value(refs_json.clone()).unwrap();
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].url, "https://rust-lang.org");
        assert_eq!(refs[1].title, None);
    }

    #[test]
    fn kimi_ref_serde_deterministic() {
        let ref1 = KimiRef {
            index: 1,
            url: "https://example.com".into(),
            title: Some("Example".into()),
        };
        let json1 = serde_json::to_string(&ref1).unwrap();
        let json2 = serde_json::to_string(&ref1).unwrap();
        assert_eq!(json1, json2);
    }

    #[test]
    fn kimi_ref_without_title_omits_field() {
        let r = KimiRef {
            index: 1,
            url: "https://example.com".into(),
            title: None,
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(!json.contains("title"));
    }

    #[test]
    fn use_search_flag_on_request() {
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("Search for Rust")])
            .use_search(true)
            .build();
        assert_eq!(req.use_search, Some(true));
    }

    #[test]
    fn kimi_config_default_has_k1_reasoning_none() {
        let cfg = KimiConfig::default();
        assert!(cfg.use_k1_reasoning.is_none());
    }

    #[test]
    fn kimi_config_default_base_url() {
        let cfg = KimiConfig::default();
        assert_eq!(cfg.base_url, "https://api.moonshot.cn/v1");
    }

    #[test]
    fn map_work_order_with_k1_reasoning_enables_search() {
        let wo = WorkOrderBuilder::new("test").build();
        let cfg = KimiConfig {
            use_k1_reasoning: Some(true),
            ..Default::default()
        };
        let req = dialect::map_work_order(&wo, &cfg);
        assert_eq!(req.use_search, Some(true));
    }

    #[test]
    fn map_work_order_without_k1_reasoning_no_search() {
        let wo = WorkOrderBuilder::new("test").build();
        let cfg = KimiConfig::default();
        let req = dialect::map_work_order(&wo, &cfg);
        assert!(req.use_search.is_none());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 6: Token Counting and Context Window Handling
// ═══════════════════════════════════════════════════════════════════════════

mod token_counting_context_window {
    use super::*;

    #[test]
    fn extract_usage_from_response() {
        let resp = KimiResponse {
            id: "cmpl-usage".into(),
            model: "moonshot-v1-8k".into(),
            choices: vec![KimiChoice {
                index: 0,
                message: KimiResponseMessage {
                    role: "assistant".into(),
                    content: Some("hi".into()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: Some(KimiUsage {
                prompt_tokens: 100,
                completion_tokens: 50,
                total_tokens: 150,
            }),
            refs: None,
        };
        let usage = dialect::extract_usage(&resp);
        let m = usage.unwrap();
        assert_eq!(m.get("prompt_tokens").unwrap().as_u64(), Some(100));
        assert_eq!(m.get("completion_tokens").unwrap().as_u64(), Some(50));
        assert_eq!(m.get("total_tokens").unwrap().as_u64(), Some(150));
    }

    #[test]
    fn extract_usage_returns_none_when_absent() {
        let resp = simple_kimi_response("hi");
        assert!(dialect::extract_usage(&resp).is_none());
    }

    #[test]
    fn usage_to_ir_converts_correctly() {
        let usage = KimiUsage {
            prompt_tokens: 200,
            completion_tokens: 80,
            total_tokens: 280,
        };
        let ir = lowering::usage_to_ir(&usage);
        assert_eq!(ir.input_tokens, 200);
        assert_eq!(ir.output_tokens, 80);
        assert_eq!(ir.total_tokens, 280);
    }

    #[test]
    fn ir_usage_to_shim_usage() {
        let ir = IrUsage::from_io(300, 100);
        let usage = ir_usage_to_usage(&ir);
        assert_eq!(usage.prompt_tokens, 300);
        assert_eq!(usage.completion_tokens, 100);
        assert_eq!(usage.total_tokens, 400);
    }

    #[test]
    fn kimi_usage_serde_roundtrip() {
        let u = KimiUsage {
            prompt_tokens: 42,
            completion_tokens: 17,
            total_tokens: 59,
        };
        let json = serde_json::to_string(&u).unwrap();
        let back: KimiUsage = serde_json::from_str(&json).unwrap();
        assert_eq!(u, back);
    }

    #[test]
    fn usage_in_btreemap_is_deterministic() {
        let resp = KimiResponse {
            id: "cmpl-det".into(),
            model: "moonshot-v1-8k".into(),
            choices: vec![],
            usage: Some(KimiUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            }),
            refs: None,
        };
        let u1 = dialect::extract_usage(&resp).unwrap();
        let u2 = dialect::extract_usage(&resp).unwrap();
        let j1 = serde_json::to_string(&u1).unwrap();
        let j2 = serde_json::to_string(&u2).unwrap();
        assert_eq!(j1, j2);
    }

    #[tokio::test]
    async fn client_tracks_token_usage() {
        let usage = UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            cache_read_tokens: None,
            cache_write_tokens: None,
            request_units: None,
            estimated_cost_usd: None,
        };
        let events = vec![assistant_msg_event("response")];
        let client = KimiClient::new("moonshot-v1-8k")
            .with_processor(make_processor_with_usage(events, usage));
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("test")])
            .build();
        let resp = client.create(req).await.unwrap();
        let u = resp.usage.unwrap();
        assert_eq!(u.prompt_tokens, 100);
        assert_eq!(u.completion_tokens, 50);
        assert_eq!(u.total_tokens, 150);
    }

    #[test]
    fn kimi_config_default_max_tokens() {
        let cfg = KimiConfig::default();
        assert_eq!(cfg.max_tokens, Some(4096));
    }

    #[test]
    fn model_8k_maps_correctly() {
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![Message::user("test")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-8k"));
    }

    #[test]
    fn model_32k_maps_correctly() {
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-32k")
            .messages(vec![Message::user("test")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-32k"));
    }

    #[test]
    fn model_128k_maps_correctly() {
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-128k")
            .messages(vec![Message::user("test")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-128k"));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 7: Kimi→OpenAI Compatibility
// ═══════════════════════════════════════════════════════════════════════════

mod kimi_openai_compatibility {
    use super::*;

    #[test]
    fn kimi_to_openai_tool_use_lossless() {
        let registry = known_rules();
        let rule = registry.lookup(Dialect::Kimi, Dialect::OpenAi, features::TOOL_USE);
        assert!(rule.is_some());
        assert!(rule.unwrap().fidelity.is_lossless());
    }

    #[test]
    fn openai_to_kimi_tool_use_lossless() {
        let registry = known_rules();
        let rule = registry.lookup(Dialect::OpenAi, Dialect::Kimi, features::TOOL_USE);
        assert!(rule.is_some());
        assert!(rule.unwrap().fidelity.is_lossless());
    }

    #[test]
    fn kimi_to_openai_streaming_lossless() {
        let registry = known_rules();
        let rule = registry.lookup(Dialect::Kimi, Dialect::OpenAi, features::STREAMING);
        assert!(rule.is_some());
        assert!(rule.unwrap().fidelity.is_lossless());
    }

    #[test]
    fn kimi_to_claude_tool_use_lossless() {
        let registry = known_rules();
        let rule = registry.lookup(Dialect::Kimi, Dialect::Claude, features::TOOL_USE);
        assert!(rule.is_some());
        assert!(rule.unwrap().fidelity.is_lossless());
    }

    #[test]
    fn kimi_to_gemini_tool_use_lossless() {
        let registry = known_rules();
        let rule = registry.lookup(Dialect::Kimi, Dialect::Gemini, features::TOOL_USE);
        assert!(rule.is_some());
        assert!(rule.unwrap().fidelity.is_lossless());
    }

    #[test]
    fn kimi_to_codex_tool_use_lossy() {
        let registry = known_rules();
        let rule = registry.lookup(Dialect::Kimi, Dialect::Codex, features::TOOL_USE);
        assert!(rule.is_some());
        assert!(matches!(
            rule.unwrap().fidelity,
            Fidelity::LossyLabeled { .. }
        ));
    }

    #[test]
    fn kimi_to_copilot_tool_use_lossless() {
        let registry = known_rules();
        let rule = registry.lookup(Dialect::Kimi, Dialect::Copilot, features::TOOL_USE);
        assert!(rule.is_some());
        assert!(rule.unwrap().fidelity.is_lossless());
    }

    #[test]
    fn kimi_self_mapping_all_lossless() {
        let registry = known_rules();
        for feat in [
            features::TOOL_USE,
            features::STREAMING,
            features::THINKING,
            features::IMAGE_INPUT,
            features::CODE_EXEC,
        ] {
            let rule = registry.lookup(Dialect::Kimi, Dialect::Kimi, feat);
            assert!(rule.is_some(), "missing self-rule for {feat}");
            assert!(
                rule.unwrap().fidelity.is_lossless(),
                "not lossless for {feat}"
            );
        }
    }

    #[test]
    fn kimi_thinking_cross_dialect_is_lossy() {
        let registry = known_rules();
        for target in [
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
        ] {
            let rule = registry.lookup(Dialect::Kimi, target, features::THINKING);
            assert!(rule.is_some(), "missing rule Kimi->{target:?} thinking");
            assert!(
                matches!(rule.unwrap().fidelity, Fidelity::LossyLabeled { .. }),
                "expected lossy for Kimi->{target:?} thinking"
            );
        }
    }

    #[test]
    fn kimi_image_input_unsupported() {
        let registry = known_rules();
        for target in [
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
        ] {
            let rule = registry.lookup(Dialect::Kimi, target, features::IMAGE_INPUT);
            assert!(rule.is_some());
            assert!(rule.unwrap().fidelity.is_unsupported());
        }
    }

    #[test]
    fn kimi_code_exec_unsupported() {
        let registry = known_rules();
        for target in [
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
            Dialect::Copilot,
        ] {
            let rule = registry.lookup(Dialect::Kimi, target, features::CODE_EXEC);
            assert!(rule.is_some());
            assert!(rule.unwrap().fidelity.is_unsupported());
        }
    }

    #[test]
    fn validate_kimi_to_openai_features() {
        let registry = known_rules();
        let feats = vec!["tool_use".into(), "streaming".into()];
        let results = validate_mapping(&registry, Dialect::Kimi, Dialect::OpenAi, &feats);
        assert_eq!(results.len(), 2);
        for v in &results {
            assert!(v.fidelity.is_lossless());
            assert!(v.errors.is_empty());
        }
    }

    #[test]
    fn validate_kimi_unknown_feature() {
        let registry = known_rules();
        let feats = vec!["nonexistent_feature".into()];
        let results = validate_mapping(&registry, Dialect::Kimi, Dialect::OpenAi, &feats);
        assert_eq!(results.len(), 1);
        assert!(results[0].fidelity.is_unsupported());
    }

    #[test]
    fn rank_targets_from_kimi() {
        let registry = known_rules();
        let feats = &[features::TOOL_USE, features::STREAMING];
        let ranked = registry.rank_targets(Dialect::Kimi, feats);
        // OpenAI should be lossless for both → top ranked
        let openai_rank = ranked.iter().find(|(d, _)| *d == Dialect::OpenAi);
        assert!(openai_rank.is_some());
        assert_eq!(openai_rank.unwrap().1, 2); // 2 lossless features
    }

    #[test]
    fn kimi_openai_message_format_compatible() {
        // Kimi uses same message format as OpenAI: role/content/tool_calls/tool_call_id
        let msg = kimi_msg("user", Some("Hello"));
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json.get("role").unwrap().as_str(), Some("user"));
        assert_eq!(json.get("content").unwrap().as_str(), Some("Hello"));
    }

    #[test]
    fn kimi_response_shape_matches_openai() {
        let resp = simple_kimi_response("Hello");
        let json = serde_json::to_value(&resp).unwrap();
        // OpenAI-compatible fields
        assert!(json.get("id").is_some());
        assert!(json.get("model").is_some());
        assert!(json.get("choices").is_some());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 8: Error Type Mapping
// ═══════════════════════════════════════════════════════════════════════════

mod error_type_mapping {
    use super::*;

    #[tokio::test]
    async fn error_event_maps_to_response_content() {
        let events = vec![error_event("rate limit exceeded")];
        let client = KimiClient::new("moonshot-v1-8k").with_processor(make_processor(events));
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("test")])
            .build();
        let resp = client.create(req).await.unwrap();
        let content = resp.choices[0].message.content.as_deref().unwrap();
        assert!(content.contains("rate limit exceeded"));
    }

    #[tokio::test]
    async fn no_processor_create_returns_error() {
        let client = KimiClient::new("moonshot-v1-8k");
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("test")])
            .build();
        let result = client.create(req).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn no_processor_stream_returns_error() {
        let client = KimiClient::new("moonshot-v1-8k");
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("test")])
            .build();
        let result = client.create_stream(req).await;
        assert!(result.is_err());
    }

    #[test]
    fn mapping_error_feature_unsupported() {
        let err = MappingError::FeatureUnsupported {
            feature: "code_exec".into(),
            from: Dialect::Kimi,
            to: Dialect::OpenAi,
        };
        let msg = err.to_string();
        assert!(msg.contains("code_exec"));
        assert!(msg.contains("unsupported"));
    }

    #[test]
    fn mapping_error_fidelity_loss() {
        let err = MappingError::FidelityLoss {
            feature: "thinking".into(),
            warning: "Kimi does not have native thinking blocks".into(),
        };
        assert!(err.to_string().contains("thinking"));
    }

    #[test]
    fn mapping_error_dialect_mismatch() {
        let err = MappingError::DialectMismatch {
            from: Dialect::Kimi,
            to: Dialect::Codex,
        };
        assert!(err.to_string().contains("mismatch"));
    }

    #[test]
    fn mapping_error_invalid_input() {
        let err = MappingError::InvalidInput {
            reason: "empty model name".into(),
        };
        assert!(err.to_string().contains("empty model name"));
    }

    #[test]
    fn error_event_in_response_has_stop_finish_reason() {
        let events = vec![error_event("internal error")];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn validate_empty_feature_name() {
        let registry = known_rules();
        let feats = vec!["".into()];
        let results = validate_mapping(&registry, Dialect::Kimi, Dialect::OpenAi, &feats);
        assert_eq!(results.len(), 1);
        assert!(results[0].fidelity.is_unsupported());
        assert!(!results[0].errors.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 9: Multi-Modal Content Handling
// ═══════════════════════════════════════════════════════════════════════════

mod multi_modal_content {
    use super::*;

    #[test]
    fn kimi_image_input_mapping_unsupported_all_targets() {
        let registry = known_rules();
        for target in Dialect::all() {
            if *target == Dialect::Kimi {
                continue;
            }
            let rule = registry.lookup(Dialect::Kimi, *target, features::IMAGE_INPUT);
            assert!(
                rule.is_some(),
                "missing image_input rule for Kimi->{target:?}"
            );
            assert!(
                rule.unwrap().fidelity.is_unsupported(),
                "expected unsupported for Kimi->{target:?} image_input"
            );
        }
    }

    #[test]
    fn text_only_message_ir_roundtrip() {
        let msgs = vec![kimi_msg("user", Some("Just text, no images"))];
        let conv = lowering::to_ir(&msgs);
        assert_eq!(conv.messages.len(), 1);
        assert_eq!(conv.messages[0].text_content(), "Just text, no images");
        let back = lowering::from_ir(&conv);
        assert_eq!(back[0].content.as_deref(), Some("Just text, no images"));
    }

    #[test]
    fn empty_content_message() {
        let msgs = vec![KimiMessage {
            role: "assistant".into(),
            content: None,
            tool_call_id: None,
            tool_calls: None,
        }];
        let conv = lowering::to_ir(&msgs);
        assert!(conv.messages[0].content.is_empty());
    }

    #[test]
    fn kimi_response_no_content_no_tool_calls_produces_no_events() {
        let resp = KimiResponse {
            id: "cmpl-empty".into(),
            model: "moonshot-v1-8k".into(),
            choices: vec![KimiChoice {
                index: 0,
                message: KimiResponseMessage {
                    role: "assistant".into(),
                    content: None,
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: None,
            refs: None,
        };
        let events = dialect::map_response(&resp);
        assert!(events.is_empty());
    }

    #[test]
    fn kimi_response_empty_string_content_no_events() {
        let resp = KimiResponse {
            id: "cmpl-empty-str".into(),
            model: "moonshot-v1-8k".into(),
            choices: vec![KimiChoice {
                index: 0,
                message: KimiResponseMessage {
                    role: "assistant".into(),
                    content: Some(String::new()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: None,
            refs: None,
        };
        let events = dialect::map_response(&resp);
        assert!(events.is_empty());
    }

    #[test]
    fn kimi_message_json_preserves_btreemap_order() {
        let mut ext = BTreeMap::new();
        ext.insert("a_key".to_string(), json!("value_a"));
        ext.insert("b_key".to_string(), json!("value_b"));
        ext.insert("c_key".to_string(), json!("value_c"));
        let json1 = serde_json::to_string(&ext).unwrap();
        let json2 = serde_json::to_string(&ext).unwrap();
        assert_eq!(json1, json2);
        assert!(json1.find("a_key").unwrap() < json1.find("b_key").unwrap());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 10: Conversation History Format
// ═══════════════════════════════════════════════════════════════════════════

mod conversation_history_format {
    use super::*;

    #[test]
    fn single_user_message_ir_roundtrip() {
        let msgs = vec![Message::user("Hello")];
        let conv = messages_to_ir(&msgs);
        assert_eq!(conv.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::User);
        let back = ir_to_messages(&conv);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].role, "user");
        assert_eq!(back[0].content.as_deref(), Some("Hello"));
    }

    #[test]
    fn system_user_assistant_roundtrip() {
        let msgs = vec![
            Message::system("Be concise."),
            Message::user("Hello"),
            Message::assistant("Hi!"),
        ];
        let conv = messages_to_ir(&msgs);
        assert_eq!(conv.len(), 3);
        let back = ir_to_messages(&conv);
        assert_eq!(back[0].role, "system");
        assert_eq!(back[0].content.as_deref(), Some("Be concise."));
        assert_eq!(back[1].role, "user");
        assert_eq!(back[2].role, "assistant");
    }

    #[test]
    fn tool_use_conversation_flow_roundtrip() {
        let tc = make_kimi_tool_call("call_1", "web_search", r#"{"q":"rust"}"#);
        let msgs = vec![
            Message::user("Search for Rust"),
            Message::assistant_with_tool_calls(vec![tc]),
            Message::tool("call_1", "Results: Rust is a systems language"),
            Message::assistant("Here are the results about Rust."),
        ];
        let conv = messages_to_ir(&msgs);
        assert_eq!(conv.len(), 4);
        assert_eq!(conv.messages[0].role, IrRole::User);
        assert_eq!(conv.messages[1].role, IrRole::Assistant);
        assert_eq!(conv.messages[2].role, IrRole::Tool);
        assert_eq!(conv.messages[3].role, IrRole::Assistant);

        let back = ir_to_messages(&conv);
        assert_eq!(back.len(), 4);
        assert_eq!(back[2].tool_call_id.as_deref(), Some("call_1"));
    }

    #[test]
    fn multi_turn_complex_conversation() {
        let msgs = vec![
            kimi_msg("system", Some("You are a helpful assistant.")),
            kimi_msg("user", Some("What is Rust?")),
            kimi_msg("assistant", Some("Rust is a systems programming language.")),
            kimi_msg("user", Some("Tell me more about ownership.")),
            kimi_msg(
                "assistant",
                Some("Ownership is Rust's memory management model."),
            ),
        ];
        let conv = lowering::to_ir(&msgs);
        assert_eq!(conv.len(), 5);
        let back = lowering::from_ir(&conv);
        assert_eq!(back.len(), 5);
        for (orig, roundtripped) in msgs.iter().zip(back.iter()) {
            assert_eq!(orig.role, roundtripped.role);
            assert_eq!(orig.content, roundtripped.content);
        }
    }

    #[test]
    fn empty_conversation() {
        let conv = lowering::to_ir(&[]);
        assert!(conv.is_empty());
        let back = lowering::from_ir(&conv);
        assert!(back.is_empty());
    }

    #[test]
    fn unknown_role_defaults_to_user() {
        let msgs = vec![kimi_msg("developer", Some("hi"))];
        let conv = lowering::to_ir(&msgs);
        assert_eq!(conv.messages[0].role, IrRole::User);
    }

    #[test]
    fn kimi_role_display() {
        assert_eq!(KimiRole::System.to_string(), "system");
        assert_eq!(KimiRole::User.to_string(), "user");
        assert_eq!(KimiRole::Assistant.to_string(), "assistant");
        assert_eq!(KimiRole::Tool.to_string(), "tool");
    }

    #[test]
    fn kimi_role_serde_roundtrip() {
        for role in [
            KimiRole::System,
            KimiRole::User,
            KimiRole::Assistant,
            KimiRole::Tool,
        ] {
            let json = serde_json::to_string(&role).unwrap();
            let back: KimiRole = serde_json::from_str(&json).unwrap();
            assert_eq!(role, back);
        }
    }

    #[test]
    fn response_to_ir_extracts_assistant_message() {
        let resp = simple_kimi_response("Hello world");
        let conv = response_to_ir(&resp);
        assert_eq!(conv.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::Assistant);
        assert_eq!(conv.messages[0].text_content(), "Hello world");
    }

    #[test]
    fn response_to_ir_extracts_tool_calls() {
        let resp = KimiResponse {
            id: "cmpl-tc".into(),
            model: "moonshot-v1-8k".into(),
            choices: vec![KimiChoice {
                index: 0,
                message: KimiResponseMessage {
                    role: "assistant".into(),
                    content: None,
                    tool_calls: Some(vec![make_kimi_tool_call(
                        "call_1",
                        "search",
                        r#"{"q":"test"}"#,
                    )]),
                },
                finish_reason: Some("tool_calls".into()),
            }],
            usage: None,
            refs: None,
        };
        let conv = response_to_ir(&resp);
        assert_eq!(conv.messages[0].role, IrRole::Assistant);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolUse { name, .. } => assert_eq!(name, "search"),
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn dialect_version_constant() {
        assert_eq!(dialect::DIALECT_VERSION, "kimi/v0.1");
    }

    #[test]
    fn default_model_constant() {
        assert_eq!(dialect::DEFAULT_MODEL, "moonshot-v1-8k");
    }

    #[tokio::test]
    async fn full_pipeline_request_to_response() {
        let events = vec![assistant_msg_event("The answer is 42.")];
        let client = KimiClient::new("moonshot-v1-8k").with_processor(make_processor(events));
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![
                Message::system("Answer precisely."),
                Message::user("What is the meaning of life?"),
            ])
            .temperature(0.0)
            .max_tokens(100)
            .build();

        let wo = request_to_work_order(&req);
        assert_eq!(wo.task, "What is the meaning of life?");
        assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-8k"));

        let resp = client.create(req).await.unwrap();
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("The answer is 42.")
        );
        assert_eq!(resp.model, "moonshot-v1-8k");
    }

    #[test]
    fn receipt_to_response_preserves_model() {
        let events = vec![assistant_msg_event("ok")];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "moonshot-v1-128k");
        assert_eq!(resp.model, "moonshot-v1-128k");
    }

    #[test]
    fn receipt_to_response_id_contains_run_id() {
        let events = vec![assistant_msg_event("ok")];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
        assert!(resp.id.starts_with("cmpl-"));
    }
}
