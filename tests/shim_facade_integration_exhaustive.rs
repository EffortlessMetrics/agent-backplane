#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Exhaustive integration tests for all six ABP shim facade crates.
//!
//! Validates request→WorkOrder, Receipt→response, field mapping, error handling,
//! streaming events, cross-shim roundtrips, and canonical contract compliance.

use abp_core::ir::{IrConversation, IrRole, IrUsage};
use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, ExecutionMode, Outcome, Receipt,
    RunMetadata, UsageNormalized, WorkOrder,
};
use chrono::Utc;
use serde_json::json;
use tokio_stream::StreamExt;

// ═══════════════════════════════════════════════════════════════════════════
// Shared helpers
// ═══════════════════════════════════════════════════════════════════════════

fn assistant_event(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: text.to_string(),
        },
        ext: None,
    }
}

fn delta_event(text: &str) -> AgentEvent {
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
            tool_name: name.to_string(),
            tool_use_id: Some(id.to_string()),
            parent_tool_use_id: None,
            input,
        },
        ext: None,
    }
}

fn error_event(msg: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: msg.to_string(),
            error_code: None,
        },
        ext: None,
    }
}

fn run_completed_event() -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunCompleted {
            message: "done".to_string(),
        },
        ext: None,
    }
}

fn run_started_event() -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "started".to_string(),
        },
        ext: None,
    }
}

fn make_receipt(events: Vec<AgentEvent>) -> Receipt {
    make_receipt_with_usage(events, UsageNormalized::default())
}

fn make_receipt_with_usage(events: Vec<AgentEvent>, usage: UsageNormalized) -> Receipt {
    let now = Utc::now();
    Receipt {
        meta: RunMetadata {
            run_id: uuid::Uuid::new_v4(),
            work_order_id: uuid::Uuid::new_v4(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: now,
            finished_at: now,
            duration_ms: 0,
        },
        backend: BackendIdentity {
            id: "mock".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: Default::default(),
        mode: ExecutionMode::Mapped,
        usage_raw: serde_json::Value::Null,
        usage,
        trace: events,
        artifacts: vec![],
        verification: Default::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

fn make_receipt_with_raw_usage(
    events: Vec<AgentEvent>,
    usage: UsageNormalized,
    usage_raw: serde_json::Value,
) -> Receipt {
    let mut r = make_receipt_with_usage(events, usage);
    r.usage_raw = usage_raw;
    r
}

fn standard_usage() -> UsageNormalized {
    UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(50),
        cache_read_tokens: None,
        cache_write_tokens: None,
        request_units: None,
        estimated_cost_usd: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  1. OpenAI shim tests
// ═══════════════════════════════════════════════════════════════════════════

mod openai {
    use super::*;
    use abp_shim_openai::*;

    fn make_processor(events: Vec<AgentEvent>) -> ProcessFn {
        Box::new(move |_wo| mock_receipt(events.clone()))
    }

    fn make_processor_with_usage(events: Vec<AgentEvent>, usage: UsageNormalized) -> ProcessFn {
        Box::new(move |_wo| mock_receipt_with_usage(events.clone(), usage.clone()))
    }

    // ── Request → WorkOrder ────────────────────────────────────────────

    #[test]
    fn request_to_work_order_extracts_task() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message::user("Explain Rust")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.task, "Explain Rust");
    }

    #[test]
    fn request_to_work_order_uses_last_user_message() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![
                Message::user("First"),
                Message::assistant("Reply"),
                Message::user("Second question"),
            ])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.task, "Second question");
    }

    #[test]
    fn request_to_work_order_sets_model() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4-turbo")
            .messages(vec![Message::user("Hi")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4-turbo"));
    }

    #[test]
    fn request_to_work_order_preserves_temperature() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message::user("Hi")])
            .temperature(0.7)
            .build();
        let wo = request_to_work_order(&req);
        let temp = wo.config.vendor.get("temperature").and_then(|v| v.as_f64());
        assert_eq!(temp, Some(0.7));
    }

    #[test]
    fn request_to_work_order_preserves_max_tokens() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message::user("Hi")])
            .max_tokens(512)
            .build();
        let wo = request_to_work_order(&req);
        let max = wo.config.vendor.get("max_tokens").and_then(|v| v.as_u64());
        assert_eq!(max, Some(512));
    }

    #[test]
    fn request_to_work_order_default_model() {
        let req = ChatCompletionRequest::builder()
            .messages(vec![Message::user("Hi")])
            .build();
        assert_eq!(req.model, "gpt-4o");
    }

    #[test]
    fn request_to_work_order_with_system_message() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![
                Message::system("You are helpful"),
                Message::user("Hello"),
            ])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.task, "Hello");
    }

    #[test]
    fn request_to_work_order_no_user_message_fallback() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message::system("System only")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.task, "chat completion");
    }

    // ── Receipt → Response ─────────────────────────────────────────────

    #[test]
    fn receipt_to_response_simple_text() {
        let receipt = make_receipt(vec![assistant_event("Hello world")]);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.choices[0].message.content, Some("Hello world".into()));
        assert_eq!(resp.choices[0].finish_reason, Some("stop".into()));
        assert_eq!(resp.model, "gpt-4o");
        assert_eq!(resp.object, "chat.completion");
    }

    #[test]
    fn receipt_to_response_with_tool_calls() {
        let receipt = make_receipt(vec![tool_call_event(
            "get_weather",
            "call_123",
            json!({"location": "SF"}),
        )]);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        let tc = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tc.len(), 1);
        assert_eq!(tc[0].function.name, "get_weather");
        assert_eq!(tc[0].id, "call_123");
        assert_eq!(tc[0].call_type, "function");
        assert_eq!(resp.choices[0].finish_reason, Some("tool_calls".into()));
    }

    #[test]
    fn receipt_to_response_with_error() {
        let receipt = make_receipt(vec![error_event("something broke")]);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert!(
            resp.choices[0]
                .message
                .content
                .as_ref()
                .unwrap()
                .contains("Error")
        );
    }

    #[test]
    fn receipt_to_response_usage_mapping() {
        let usage = standard_usage();
        let receipt = make_receipt_with_usage(vec![assistant_event("ok")], usage);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        let u = resp.usage.as_ref().unwrap();
        assert_eq!(u.prompt_tokens, 100);
        assert_eq!(u.completion_tokens, 50);
        assert_eq!(u.total_tokens, 150);
    }

    #[test]
    fn receipt_to_response_delta_concatenation() {
        let receipt = make_receipt(vec![delta_event("Hel"), delta_event("lo!")]);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.choices[0].message.content, Some("Hello!".into()));
    }

    #[test]
    fn receipt_to_response_id_format() {
        let receipt = make_receipt(vec![assistant_event("ok")]);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert!(resp.id.starts_with("chatcmpl-"));
    }

    // ── Streaming ──────────────────────────────────────────────────────

    #[test]
    fn stream_events_from_assistant_message() {
        let events = vec![assistant_event("Hello")];
        let stream = events_to_stream_events(&events, "gpt-4o");
        assert!(stream.len() >= 2); // at least data + stop
        assert_eq!(stream[0].choices[0].delta.content, Some("Hello".into()));
    }

    #[test]
    fn stream_events_from_delta() {
        let events = vec![delta_event("Hi"), delta_event(" there")];
        let stream = events_to_stream_events(&events, "gpt-4o");
        assert!(stream.len() >= 3);
        assert_eq!(stream[0].choices[0].delta.content, Some("Hi".into()));
        assert_eq!(stream[1].choices[0].delta.content, Some(" there".into()));
    }

    #[test]
    fn stream_events_tool_call() {
        let events = vec![tool_call_event("search", "call_1", json!({"q": "rust"}))];
        let stream = events_to_stream_events(&events, "gpt-4o");
        let tc = &stream[0].choices[0].delta.tool_calls.as_ref().unwrap()[0];
        assert_eq!(tc.function.as_ref().unwrap().name, Some("search".into()));
    }

    #[test]
    fn stream_events_has_stop_chunk() {
        let events = vec![assistant_event("done")];
        let stream = events_to_stream_events(&events, "gpt-4o");
        let last = stream.last().unwrap();
        assert_eq!(last.choices[0].finish_reason, Some("stop".into()));
    }

    // ── Client roundtrip ───────────────────────────────────────────────

    #[tokio::test]
    async fn client_roundtrip_simple() {
        let events = vec![assistant_event("Response text")];
        let client = OpenAiClient::new("gpt-4o").with_processor(make_processor(events));
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message::user("Hello")])
            .build();
        let resp = client.chat().completions().create(req).await.unwrap();
        assert_eq!(
            resp.choices[0].message.content,
            Some("Response text".into())
        );
    }

    #[tokio::test]
    async fn client_no_processor_returns_error() {
        let client = OpenAiClient::new("gpt-4o");
        let req = ChatCompletionRequest::builder()
            .messages(vec![Message::user("Hi")])
            .build();
        let result = client.chat().completions().create(req).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn client_stream_roundtrip() {
        let events = vec![assistant_event("Stream response")];
        let client = OpenAiClient::new("gpt-4o").with_processor(make_processor(events));
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message::user("Stream me")])
            .stream(true)
            .build();
        let mut stream = client
            .chat()
            .completions()
            .create_stream(req)
            .await
            .unwrap();
        let mut collected = Vec::new();
        while let Some(ev) = stream.next().await {
            collected.push(ev);
        }
        assert!(!collected.is_empty());
    }

    // ── IR roundtrip ───────────────────────────────────────────────────

    #[test]
    fn messages_to_ir_roundtrip() {
        let msgs = vec![Message::user("Hello"), Message::assistant("Hi back")];
        let ir = messages_to_ir(&msgs);
        assert!(ir.messages.iter().any(|m| m.role == IrRole::User));
        assert!(ir.messages.iter().any(|m| m.role == IrRole::Assistant));
    }

    #[test]
    fn ir_usage_conversion() {
        let ir = IrUsage {
            input_tokens: 10,
            output_tokens: 20,
            total_tokens: 30,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        };
        let usage = ir_usage_to_usage(&ir);
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 20);
        assert_eq!(usage.total_tokens, 30);
    }

    #[test]
    fn tools_to_ir_conversion() {
        let tools = vec![Tool::function(
            "search",
            "Search the web",
            json!({"type": "object", "properties": {}}),
        )];
        let ir_tools = tools_to_ir(&tools);
        assert_eq!(ir_tools.len(), 1);
        assert_eq!(ir_tools[0].name, "search");
    }

    // ── Multiple tool calls ────────────────────────────────────────────

    #[test]
    fn receipt_to_response_multiple_tool_calls() {
        let receipt = make_receipt(vec![
            tool_call_event("search", "call_1", json!({"q": "a"})),
            tool_call_event("calc", "call_2", json!({"expr": "1+1"})),
        ]);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        let tc = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tc.len(), 2);
        assert_eq!(tc[0].function.name, "search");
        assert_eq!(tc[1].function.name, "calc");
    }

    // ── Builder patterns ───────────────────────────────────────────────

    #[test]
    fn request_builder_all_fields() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o-mini")
            .messages(vec![Message::user("test")])
            .temperature(0.5)
            .max_tokens(1024)
            .stream(true)
            .stop(vec!["END".into()])
            .build();
        assert_eq!(req.model, "gpt-4o-mini");
        assert_eq!(req.temperature, Some(0.5));
        assert_eq!(req.max_tokens, Some(1024));
        assert_eq!(req.stream, Some(true));
        assert_eq!(req.stop, Some(vec!["END".into()]));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  2. Claude shim tests
// ═══════════════════════════════════════════════════════════════════════════

mod claude {
    use super::*;
    use abp_shim_claude::convert;
    use abp_shim_claude::types::{
        ClaudeContent, ClaudeMessage, ClaudeTool, ClaudeToolChoice, ClaudeUsage, ContentBlock,
        ImageSource, MessageDeltaBody, MessagesRequest, MessagesResponse, StreamDelta, StreamEvent,
    };
    use abp_shim_claude::{
        AnthropicClient, Message, MessageRequest, MessageResponse, Role, ShimError, Usage,
    };

    // ── Request → WorkOrder ────────────────────────────────────────────

    #[test]
    fn request_to_work_order_extracts_user_message() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![ClaudeMessage {
                role: "user".into(),
                content: ClaudeContent::Text("Explain monads".into()),
            }],
            max_tokens: 1024,
            system: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stream: None,
            stop_sequences: None,
            tools: None,
            tool_choice: None,
            thinking: None,
        };
        let wo = convert::to_work_order(&req);
        assert_eq!(wo.task, "Explain monads");
        assert_eq!(wo.config.model, Some("claude-sonnet-4-20250514".into()));
    }

    #[test]
    fn request_to_work_order_system_fallback() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![],
            max_tokens: 1024,
            system: Some("You are helpful".into()),
            temperature: None,
            top_p: None,
            top_k: None,
            stream: None,
            stop_sequences: None,
            tools: None,
            tool_choice: None,
            thinking: None,
        };
        let wo = convert::to_work_order(&req);
        assert_eq!(wo.task, "You are helpful");
    }

    #[test]
    fn request_to_work_order_default_task_when_empty() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![],
            max_tokens: 1024,
            system: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stream: None,
            stop_sequences: None,
            tools: None,
            tool_choice: None,
            thinking: None,
        };
        let wo = convert::to_work_order(&req);
        assert_eq!(wo.task, "Claude shim request");
    }

    #[test]
    fn request_to_work_order_preserves_temperature() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![ClaudeMessage {
                role: "user".into(),
                content: ClaudeContent::Text("Hi".into()),
            }],
            max_tokens: 1024,
            system: None,
            temperature: Some(0.3),
            top_p: None,
            top_k: None,
            stream: None,
            stop_sequences: None,
            tools: None,
            tool_choice: None,
            thinking: None,
        };
        let wo = convert::to_work_order(&req);
        let temp = wo.config.vendor.get("temperature").and_then(|v| v.as_f64());
        assert_eq!(temp, Some(0.3));
    }

    #[test]
    fn request_to_work_order_stores_max_tokens() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![ClaudeMessage {
                role: "user".into(),
                content: ClaudeContent::Text("test".into()),
            }],
            max_tokens: 2048,
            system: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stream: None,
            stop_sequences: None,
            tools: None,
            tool_choice: None,
            thinking: None,
        };
        let wo = convert::to_work_order(&req);
        let max = wo.config.vendor.get("max_tokens").and_then(|v| v.as_u64());
        assert_eq!(max, Some(2048));
    }

    // ── Receipt → Response ─────────────────────────────────────────────

    #[test]
    fn from_receipt_simple_text() {
        let receipt = make_receipt_with_raw_usage(
            vec![assistant_event("Hello from Claude")],
            UsageNormalized::default(),
            json!({"input_tokens": 10, "output_tokens": 25}),
        );
        let wo_req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![ClaudeMessage {
                role: "user".into(),
                content: ClaudeContent::Text("Hi".into()),
            }],
            max_tokens: 1024,
            system: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stream: None,
            stop_sequences: None,
            tools: None,
            tool_choice: None,
            thinking: None,
        };
        let wo = convert::to_work_order(&wo_req);
        let resp = convert::from_receipt(&receipt, &wo);
        assert_eq!(resp.role, "assistant");
        assert_eq!(resp.type_field, "message");
        assert!(!resp.content.is_empty());
    }

    #[test]
    fn from_receipt_tool_use() {
        let receipt = make_receipt_with_raw_usage(
            vec![tool_call_event("search", "tu_1", json!({"query": "rust"}))],
            UsageNormalized::default(),
            json!({"input_tokens": 5, "output_tokens": 10}),
        );
        let wo_req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![ClaudeMessage {
                role: "user".into(),
                content: ClaudeContent::Text("search for rust".into()),
            }],
            max_tokens: 1024,
            system: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stream: None,
            stop_sequences: None,
            tools: None,
            tool_choice: None,
            thinking: None,
        };
        let wo = convert::to_work_order(&wo_req);
        let resp = convert::from_receipt(&receipt, &wo);
        assert_eq!(resp.stop_reason, Some("tool_use".into()));
        assert!(
            resp.content
                .iter()
                .any(|b| matches!(b, ContentBlock::ToolUse { name, .. } if name == "search"))
        );
    }

    #[test]
    fn from_receipt_end_turn_stop_reason() {
        let receipt = make_receipt_with_raw_usage(
            vec![assistant_event("Done"), run_completed_event()],
            UsageNormalized::default(),
            json!({}),
        );
        let wo_req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![ClaudeMessage {
                role: "user".into(),
                content: ClaudeContent::Text("x".into()),
            }],
            max_tokens: 1024,
            system: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stream: None,
            stop_sequences: None,
            tools: None,
            tool_choice: None,
            thinking: None,
        };
        let wo = convert::to_work_order(&wo_req);
        let resp = convert::from_receipt(&receipt, &wo);
        assert_eq!(resp.stop_reason, Some("end_turn".into()));
    }

    #[test]
    fn from_receipt_id_format() {
        let receipt = make_receipt_with_raw_usage(
            vec![assistant_event("ok")],
            UsageNormalized::default(),
            json!({}),
        );
        let wo_req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![ClaudeMessage {
                role: "user".into(),
                content: ClaudeContent::Text("x".into()),
            }],
            max_tokens: 1024,
            system: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stream: None,
            stop_sequences: None,
            tools: None,
            tool_choice: None,
            thinking: None,
        };
        let wo = convert::to_work_order(&wo_req);
        let resp = convert::from_receipt(&receipt, &wo);
        assert!(resp.id.starts_with("msg_"));
    }

    // ── Streaming ──────────────────────────────────────────────────────

    #[test]
    fn from_agent_event_text_delta() {
        let event = delta_event("chunk");
        let se = convert::from_agent_event(&event);
        assert!(se.is_some());
        match se.unwrap() {
            StreamEvent::ContentBlockDelta { delta, .. } => match delta {
                StreamDelta::TextDelta { text } => assert_eq!(text, "chunk"),
                _ => panic!("expected TextDelta"),
            },
            _ => panic!("expected ContentBlockDelta"),
        }
    }

    #[test]
    fn from_agent_event_tool_call() {
        let event = tool_call_event("read_file", "tu_2", json!({"path": "/tmp"}));
        let se = convert::from_agent_event(&event);
        assert!(se.is_some());
        match se.unwrap() {
            abp_shim_claude::types::StreamEvent::ContentBlockStart { content_block, .. } => {
                match content_block {
                    abp_shim_claude::types::ContentBlock::ToolUse { name, .. } => {
                        assert_eq!(name, "read_file")
                    }
                    _ => panic!("expected ToolUse"),
                }
            }
            _ => panic!("expected ContentBlockStart"),
        }
    }

    #[test]
    fn from_agent_event_run_completed() {
        let event = run_completed_event();
        let se = convert::from_agent_event(&event);
        assert!(se.is_some());
        match se.unwrap() {
            StreamEvent::MessageDelta { delta, .. } => {
                assert_eq!(delta.stop_reason, Some("end_turn".into()));
            }
            _ => panic!("expected MessageDelta"),
        }
    }

    #[test]
    fn from_agent_event_unhandled_returns_none() {
        let event = run_started_event();
        let se = convert::from_agent_event(&event);
        assert!(se.is_none());
    }

    // ── Client roundtrip ───────────────────────────────────────────────

    #[tokio::test]
    async fn client_simple_completion() {
        let client = AnthropicClient::new();
        let req = MessageRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 1024,
            messages: vec![abp_shim_claude::Message {
                role: Role::User,
                content: vec![abp_shim_claude::ContentBlock::Text {
                    text: "Hello".into(),
                }],
            }],
            system: None,
            temperature: None,
            stop_sequences: None,
            thinking: None,
            stream: None,
        };
        let resp = client.create(req).await.unwrap();
        assert!(!resp.content.is_empty());
        assert_eq!(resp.role, "assistant");
    }

    #[tokio::test]
    async fn client_empty_messages_returns_error() {
        let client = AnthropicClient::new();
        let req = MessageRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 1024,
            messages: vec![],
            system: None,
            temperature: None,
            stop_sequences: None,
            thinking: None,
            stream: None,
        };
        let result = client.create(req).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn client_streaming() {
        let client = AnthropicClient::new();
        let req = MessageRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 1024,
            messages: vec![abp_shim_claude::Message {
                role: Role::User,
                content: vec![abp_shim_claude::ContentBlock::Text {
                    text: "Stream test".into(),
                }],
            }],
            system: None,
            temperature: None,
            stop_sequences: None,
            thinking: None,
            stream: None,
        };
        let mut stream = client.create_stream(req).await.unwrap();
        let mut collected = Vec::new();
        while let Some(ev) = stream.next().await {
            collected.push(ev);
        }
        assert!(!collected.is_empty());
    }

    // ── Content block conversions ──────────────────────────────────────

    #[test]
    fn content_block_text_roundtrip() {
        let block = ContentBlock::Text {
            text: "hello".into(),
        };
        let json = serde_json::to_value(&block).unwrap();
        let back: ContentBlock = serde_json::from_value(json).unwrap();
        assert_eq!(block, back);
    }

    #[test]
    fn content_block_tool_use_roundtrip() {
        let block = ContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "search".into(),
            input: json!({"q": "test"}),
        };
        let json = serde_json::to_value(&block).unwrap();
        let back: ContentBlock = serde_json::from_value(json).unwrap();
        assert_eq!(block, back);
    }

    // ── Role mapping ───────────────────────────────────────────────────

    #[test]
    fn role_mapping_user() {
        assert_eq!(convert::map_role_to_abp("user"), "user");
    }

    #[test]
    fn role_mapping_assistant() {
        assert_eq!(convert::map_role_to_abp("assistant"), "assistant");
    }

    #[test]
    fn role_mapping_unknown_defaults_user() {
        assert_eq!(convert::map_role_to_abp("unknown"), "user");
    }

    #[test]
    fn role_from_abp_system_maps_to_user() {
        assert_eq!(convert::map_role_from_abp("system"), "user");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  3. Gemini shim tests
// ═══════════════════════════════════════════════════════════════════════════

mod gemini {
    use super::*;
    use abp_shim_gemini::*;

    // ── Request → IR → WorkOrder ───────────────────────────────────────

    #[test]
    fn request_to_ir_simple_text() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("Hello Gemini")]));
        let (ir, _cfg, _safety) = request_to_ir(&req).unwrap();
        assert!(!ir.conversation.messages.is_empty());
    }

    #[test]
    fn ir_to_work_order_sets_model() {
        let req = GenerateContentRequest::new("gemini-2.5-pro")
            .add_content(Content::user(vec![Part::text("test")]));
        let (ir, cfg, _) = request_to_ir(&req).unwrap();
        let wo = ir_to_work_order(&ir, "gemini-2.5-pro", &cfg);
        assert!(wo.config.model.is_some());
    }

    #[test]
    fn ir_to_work_order_extracts_task_text() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("Tell me a joke")]));
        let (ir, cfg, _) = request_to_ir(&req).unwrap();
        let wo = ir_to_work_order(&ir, "gemini-2.5-flash", &cfg);
        assert!(wo.task.contains("Tell me a joke"));
    }

    // ── Receipt → Response ─────────────────────────────────────────────

    #[test]
    fn execute_and_convert_roundtrip() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("Count to 3")]));
        let (ir, cfg, safety) = request_to_ir(&req).unwrap();
        let wo = ir_to_work_order(&ir, "gemini-2.5-flash", &cfg);
        let receipt = execute_work_order(&wo);
        let ir_resp = receipt_to_ir(&receipt);
        let resp = ir_to_response(&ir_resp, &receipt, &cfg, &safety).unwrap();
        assert!(!resp.candidates.is_empty());
        assert!(resp.text().is_some());
    }

    #[test]
    fn usage_metadata_from_receipt() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("usage test")]));
        let (ir, cfg, _) = request_to_ir(&req).unwrap();
        let wo = ir_to_work_order(&ir, "gemini-2.5-flash", &cfg);
        let receipt = execute_work_order(&wo);
        let usage = make_usage_metadata(&receipt.usage);
        assert!(usage.is_some());
        let u = usage.unwrap();
        assert_eq!(
            u.total_token_count,
            u.prompt_token_count + u.candidates_token_count
        );
    }

    // ── Streaming ──────────────────────────────────────────────────────

    #[test]
    fn receipt_to_stream_events_has_content() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("stream test")]));
        let (ir, cfg, _) = request_to_ir(&req).unwrap();
        let wo = ir_to_work_order(&ir, "gemini-2.5-flash", &cfg);
        let receipt = execute_work_order(&wo);
        let events = receipt_to_stream_events(&receipt);
        assert!(!events.is_empty());
        assert!(events.iter().any(|e| e.text().is_some()));
    }

    // ── Part conversions ───────────────────────────────────────────────

    #[test]
    fn part_text_roundtrip() {
        let p = Part::text("hello");
        let dialect = part_to_dialect(&p);
        let back = part_from_dialect(&dialect);
        assert_eq!(p, back);
    }

    #[test]
    fn part_function_call_roundtrip() {
        let p = Part::function_call("my_fn", json!({"arg": 1}));
        let dialect = part_to_dialect(&p);
        let back = part_from_dialect(&dialect);
        assert_eq!(p, back);
    }

    #[test]
    fn part_function_response_roundtrip() {
        let p = Part::function_response("my_fn", json!({"result": "ok"}));
        let dialect = part_to_dialect(&p);
        let back = part_from_dialect(&dialect);
        assert_eq!(p, back);
    }

    #[test]
    fn part_inline_data_roundtrip() {
        let p = Part::inline_data("image/png", "base64data");
        let dialect = part_to_dialect(&p);
        let back = part_from_dialect(&dialect);
        assert_eq!(p, back);
    }

    // ── Content conversions ────────────────────────────────────────────

    #[test]
    fn content_user_roundtrip() {
        let c = Content::user(vec![Part::text("user msg")]);
        let dialect = content_to_dialect(&c);
        let back = content_from_dialect(&dialect);
        assert_eq!(c.role, back.role);
        assert_eq!(c.parts.len(), back.parts.len());
    }

    #[test]
    fn content_model_roundtrip() {
        let c = Content::model(vec![Part::text("model reply")]);
        let dialect = content_to_dialect(&c);
        let back = content_from_dialect(&dialect);
        assert_eq!(c.role, back.role);
    }

    // ── GenerateContentRequest builder ─────────────────────────────────

    #[test]
    fn request_builder_chain() {
        let req = GenerateContentRequest::new("gemini-2.5-pro")
            .add_content(Content::user(vec![Part::text("test")]))
            .generation_config(GenerationConfig {
                max_output_tokens: Some(100),
                temperature: Some(0.5),
                top_p: Some(0.9),
                top_k: Some(40),
                candidate_count: None,
                stop_sequences: None,
                response_mime_type: None,
                response_schema: None,
            });
        assert_eq!(req.model, "gemini-2.5-pro");
        assert!(req.generation_config.is_some());
    }

    // ── Client ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn gemini_client_generate() {
        let client = GeminiClient::new("gemini-2.5-flash");
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("Hello")]));
        let resp = client.generate(req).await.unwrap();
        assert!(!resp.candidates.is_empty());
    }

    #[tokio::test]
    async fn gemini_client_stream() {
        let client = GeminiClient::new("gemini-2.5-flash");
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("Stream")]));
        let mut stream = client.generate_stream(req).await.unwrap();
        let mut collected = Vec::new();
        while let Some(ev) = stream.next().await {
            collected.push(ev);
        }
        assert!(!collected.is_empty());
    }

    // ── Usage conversion ───────────────────────────────────────────────

    #[test]
    fn usage_from_ir_roundtrip() {
        let ir = IrUsage {
            input_tokens: 42,
            output_tokens: 58,
            total_tokens: 100,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        };
        let usage = usage_from_ir(&ir);
        assert_eq!(usage.prompt_token_count, 42);
        assert_eq!(usage.candidates_token_count, 58);
        assert_eq!(usage.total_token_count, 100);
    }

    #[test]
    fn usage_to_ir_roundtrip() {
        let u = UsageMetadata {
            prompt_token_count: 10,
            candidates_token_count: 20,
            total_token_count: 30,
        };
        let ir = usage_to_ir(&u);
        assert_eq!(ir.input_tokens, 10);
        assert_eq!(ir.output_tokens, 20);
        assert_eq!(ir.total_tokens, 30);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  4. Codex shim tests
// ═══════════════════════════════════════════════════════════════════════════

mod codex {
    use super::*;
    use abp_codex_sdk::dialect::{CodexInputItem, CodexRequest};
    use abp_shim_codex::*;

    fn simple_codex_request() -> CodexRequest {
        CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![codex_message("user", "Write a function")])
            .build()
    }

    fn make_processor(events: Vec<AgentEvent>) -> ProcessFn {
        Box::new(move |_wo| mock_receipt(events.clone()))
    }

    // ── Request → WorkOrder ────────────────────────────────────────────

    #[test]
    fn request_to_work_order_extracts_task() {
        let req = simple_codex_request();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.task, "Write a function");
    }

    #[test]
    fn request_to_work_order_sets_model() {
        let req = CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![codex_message("user", "test")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("codex-mini-latest"));
    }

    #[test]
    fn request_to_work_order_preserves_temperature() {
        let req = CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![codex_message("user", "test")])
            .temperature(0.8)
            .build();
        let wo = request_to_work_order(&req);
        let temp = wo.config.vendor.get("temperature").and_then(|v| v.as_f64());
        assert_eq!(temp, Some(0.8));
    }

    #[test]
    fn request_to_work_order_preserves_max_output_tokens() {
        let req = CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![codex_message("user", "test")])
            .max_output_tokens(256)
            .build();
        let wo = request_to_work_order(&req);
        let max = wo
            .config
            .vendor
            .get("max_output_tokens")
            .and_then(|v| v.as_u64());
        assert_eq!(max, Some(256));
    }

    #[test]
    fn request_to_work_order_default_model() {
        let req = CodexRequestBuilder::new()
            .input(vec![codex_message("user", "test")])
            .build();
        assert_eq!(req.model, "codex-mini-latest");
    }

    #[test]
    fn request_to_work_order_no_user_message() {
        let req = CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![codex_message("system", "You are helpful")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.task, "codex completion");
    }

    // ── Receipt → Response ─────────────────────────────────────────────

    #[test]
    fn receipt_to_response_simple_message() {
        let receipt = make_receipt(vec![assistant_event("Here is a function")]);
        let resp = receipt_to_response(&receipt, "codex-mini-latest");
        assert!(!resp.output.is_empty());
        assert_eq!(resp.model, "codex-mini-latest");
    }

    #[test]
    fn receipt_to_response_tool_call() {
        let receipt = make_receipt(vec![tool_call_event("exec", "fc_1", json!({"cmd": "ls"}))]);
        let resp = receipt_to_response(&receipt, "codex-mini-latest");
        assert!(resp.output.iter().any(|item| {
            matches!(item, abp_codex_sdk::dialect::CodexResponseItem::FunctionCall { name, .. } if name == "exec")
        }));
    }

    #[test]
    fn receipt_to_response_error_event() {
        let receipt = make_receipt(vec![error_event("boom")]);
        let resp = receipt_to_response(&receipt, "codex-mini-latest");
        assert!(!resp.output.is_empty());
    }

    #[test]
    fn receipt_to_response_id_format() {
        let receipt = make_receipt(vec![assistant_event("ok")]);
        let resp = receipt_to_response(&receipt, "codex-mini-latest");
        assert!(resp.id.starts_with("resp_"));
    }

    // ── Streaming ──────────────────────────────────────────────────────

    #[test]
    fn stream_events_from_assistant() {
        let events = vec![assistant_event("chunk1")];
        let stream = events_to_stream_events(&events, "codex-mini-latest");
        assert!(!stream.is_empty());
    }

    #[test]
    fn stream_events_from_delta() {
        let events = vec![delta_event("part1"), delta_event("part2")];
        let stream = events_to_stream_events(&events, "codex-mini-latest");
        assert!(stream.len() >= 2);
    }

    // ── Client roundtrip ───────────────────────────────────────────────

    #[tokio::test]
    async fn client_roundtrip() {
        let events = vec![assistant_event("Code output")];
        let client = CodexClient::new("codex-mini-latest").with_processor(make_processor(events));
        let req = simple_codex_request();
        let resp = client.create(req).await.unwrap();
        assert!(!resp.output.is_empty());
    }

    #[tokio::test]
    async fn client_no_processor_error() {
        let client = CodexClient::new("codex-mini-latest");
        let req = simple_codex_request();
        let result = client.create(req).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn client_stream() {
        let events = vec![assistant_event("streamed")];
        let client = CodexClient::new("codex-mini-latest").with_processor(make_processor(events));
        let req = simple_codex_request();
        let mut stream = client.create_stream(req).await.unwrap();
        let mut count = 0;
        while let Some(_ev) = stream.next().await {
            count += 1;
        }
        assert!(count > 0);
    }

    // ── IR roundtrip ───────────────────────────────────────────────────

    #[test]
    fn request_to_ir_has_user_message() {
        let req = simple_codex_request();
        let ir = request_to_ir(&req);
        assert!(ir.messages.iter().any(|m| m.role == IrRole::User));
    }

    #[test]
    fn ir_usage_to_usage_conversion() {
        let ir = IrUsage {
            input_tokens: 15,
            output_tokens: 25,
            total_tokens: 40,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        };
        let u = ir_usage_to_usage(&ir);
        assert_eq!(u.input_tokens, 15);
        assert_eq!(u.output_tokens, 25);
        assert_eq!(u.total_tokens, 40);
    }

    // ── Builder ────────────────────────────────────────────────────────

    #[test]
    fn builder_all_fields() {
        let req = CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![codex_message("user", "test")])
            .temperature(0.5)
            .max_output_tokens(200)
            .build();
        assert_eq!(req.model, "codex-mini-latest");
        assert_eq!(req.temperature, Some(0.5));
        assert_eq!(req.max_output_tokens, Some(200));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  5. Kimi shim tests
// ═══════════════════════════════════════════════════════════════════════════

mod kimi {
    use super::*;
    use abp_shim_kimi::*;

    fn simple_kimi_request() -> abp_kimi_sdk::dialect::KimiRequest {
        KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![Message::user("What is Rust?")])
            .build()
    }

    fn make_processor(events: Vec<AgentEvent>) -> ProcessFn {
        Box::new(move |_wo| mock_receipt(events.clone()))
    }

    // ── Request → WorkOrder ────────────────────────────────────────────

    #[test]
    fn request_to_work_order_extracts_task() {
        let req = simple_kimi_request();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.task, "What is Rust?");
    }

    #[test]
    fn request_to_work_order_sets_model() {
        let req = simple_kimi_request();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-8k"));
    }

    #[test]
    fn request_to_work_order_preserves_temperature() {
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![Message::user("test")])
            .temperature(0.6)
            .build();
        let wo = request_to_work_order(&req);
        let temp = wo.config.vendor.get("temperature").and_then(|v| v.as_f64());
        assert_eq!(temp, Some(0.6));
    }

    #[test]
    fn request_to_work_order_preserves_max_tokens() {
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![Message::user("test")])
            .max_tokens(512)
            .build();
        let wo = request_to_work_order(&req);
        let max = wo.config.vendor.get("max_tokens").and_then(|v| v.as_u64());
        assert_eq!(max, Some(512));
    }

    #[test]
    fn request_to_work_order_default_model() {
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("test")])
            .build();
        assert_eq!(req.model, "moonshot-v1-8k");
    }

    #[test]
    fn request_to_work_order_no_user_message() {
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![Message::system("system prompt")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.task, "kimi completion");
    }

    // ── Receipt → Response ─────────────────────────────────────────────

    #[test]
    fn receipt_to_response_simple_text() {
        let receipt = make_receipt(vec![assistant_event("Kimi reply")]);
        let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
        assert!(resp.choices[0].message.content.is_some());
        assert_eq!(resp.model, "moonshot-v1-8k");
    }

    #[test]
    fn receipt_to_response_tool_call() {
        let receipt = make_receipt(vec![tool_call_event(
            "web_search",
            "call_k1",
            json!({"q": "test"}),
        )]);
        let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
        let tc = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tc[0].function.name, "web_search");
        assert_eq!(resp.choices[0].finish_reason, Some("tool_calls".into()));
    }

    #[test]
    fn receipt_to_response_error_event() {
        let receipt = make_receipt(vec![error_event("kimi error")]);
        let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
        assert!(
            resp.choices[0]
                .message
                .content
                .as_ref()
                .unwrap()
                .contains("Error")
        );
    }

    #[test]
    fn receipt_to_response_delta_concat() {
        let receipt = make_receipt(vec![delta_event("A"), delta_event("B")]);
        let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
        assert_eq!(resp.choices[0].message.content, Some("AB".into()));
    }

    // ── Streaming ──────────────────────────────────────────────────────

    #[test]
    fn stream_chunks_from_events() {
        let events = vec![assistant_event("chunk")];
        let chunks = events_to_stream_chunks(&events, "moonshot-v1-8k");
        assert!(!chunks.is_empty());
    }

    #[test]
    fn stream_chunks_from_deltas() {
        let events = vec![delta_event("p1"), delta_event("p2")];
        let chunks = events_to_stream_chunks(&events, "moonshot-v1-8k");
        assert!(chunks.len() >= 2);
    }

    // ── Client roundtrip ───────────────────────────────────────────────

    #[tokio::test]
    async fn client_roundtrip() {
        let events = vec![assistant_event("Kimi response")];
        let client = KimiClient::new("moonshot-v1-8k").with_processor(make_processor(events));
        let req = simple_kimi_request();
        let resp = client.create(req).await.unwrap();
        assert!(resp.choices[0].message.content.is_some());
    }

    #[tokio::test]
    async fn client_no_processor_error() {
        let client = KimiClient::new("moonshot-v1-8k");
        let req = simple_kimi_request();
        let result = client.create(req).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn client_stream() {
        let events = vec![assistant_event("streamed")];
        let client = KimiClient::new("moonshot-v1-8k").with_processor(make_processor(events));
        let req = simple_kimi_request();
        let mut stream = client.create_stream(req).await.unwrap();
        let mut count = 0;
        while let Some(_c) = stream.next().await {
            count += 1;
        }
        assert!(count > 0);
    }

    // ── IR roundtrip ───────────────────────────────────────────────────

    #[test]
    fn messages_to_ir_roundtrip() {
        let msgs = vec![Message::user("Hello"), Message::assistant("Hi")];
        let ir = messages_to_ir(&msgs);
        assert!(ir.messages.iter().any(|m| m.role == IrRole::User));
        assert!(ir.messages.iter().any(|m| m.role == IrRole::Assistant));
    }

    #[test]
    fn ir_to_messages_roundtrip() {
        let msgs = vec![Message::user("question")];
        let ir = messages_to_ir(&msgs);
        let back = ir_to_messages(&ir);
        assert!(!back.is_empty());
        assert_eq!(back[0].role, "user");
    }

    #[test]
    fn ir_usage_conversion() {
        let ir = IrUsage {
            input_tokens: 30,
            output_tokens: 40,
            total_tokens: 70,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        };
        let u = ir_usage_to_usage(&ir);
        assert_eq!(u.prompt_tokens, 30);
        assert_eq!(u.completion_tokens, 40);
        assert_eq!(u.total_tokens, 70);
    }

    // ── Builder ────────────────────────────────────────────────────────

    #[test]
    fn builder_all_fields() {
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-32k")
            .messages(vec![Message::user("test")])
            .temperature(0.9)
            .max_tokens(1000)
            .stream(true)
            .use_search(true)
            .build();
        assert_eq!(req.model, "moonshot-v1-32k");
        assert_eq!(req.temperature, Some(0.9));
        assert_eq!(req.max_tokens, Some(1000));
        assert_eq!(req.stream, Some(true));
        assert_eq!(req.use_search, Some(true));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  6. Copilot shim tests
// ═══════════════════════════════════════════════════════════════════════════

mod copilot {
    use super::*;
    use abp_shim_copilot::*;

    fn simple_copilot_request() -> abp_copilot_sdk::dialect::CopilotRequest {
        CopilotRequestBuilder::new()
            .model("gpt-4o")
            .messages(vec![Message::user("Copilot question")])
            .build()
    }

    fn make_processor(events: Vec<AgentEvent>) -> ProcessFn {
        Box::new(move |_wo| mock_receipt(events.clone()))
    }

    // ── Request → WorkOrder ────────────────────────────────────────────

    #[test]
    fn request_to_work_order_extracts_task() {
        let req = simple_copilot_request();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.task, "Copilot question");
    }

    #[test]
    fn request_to_work_order_sets_model() {
        let req = CopilotRequestBuilder::new()
            .model("gpt-4o-mini")
            .messages(vec![Message::user("test")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4o-mini"));
    }

    #[test]
    fn request_to_work_order_default_model() {
        let req = CopilotRequestBuilder::new()
            .messages(vec![Message::user("test")])
            .build();
        assert_eq!(req.model, "gpt-4o");
    }

    #[test]
    fn request_to_work_order_no_user_message() {
        let req = CopilotRequestBuilder::new()
            .model("gpt-4o")
            .messages(vec![Message::system("system")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.task, "copilot completion");
    }

    // ── Receipt → Response ─────────────────────────────────────────────

    #[test]
    fn receipt_to_response_simple() {
        let receipt = make_receipt(vec![assistant_event("Copilot answer")]);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.message, "Copilot answer");
    }

    #[test]
    fn receipt_to_response_delta_concat() {
        let receipt = make_receipt(vec![delta_event("Hello "), delta_event("world")]);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.message, "Hello world");
    }

    #[test]
    fn receipt_to_response_tool_call() {
        let receipt = make_receipt(vec![tool_call_event(
            "run_cmd",
            "tc_1",
            json!({"cmd": "echo"}),
        )]);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert!(resp.function_call.is_some());
        let fc = resp.function_call.unwrap();
        assert_eq!(fc.name, "run_cmd");
    }

    #[test]
    fn receipt_to_response_error_event() {
        let receipt = make_receipt(vec![error_event("copilot error")]);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.copilot_errors.len(), 1);
        assert!(resp.copilot_errors[0].message.contains("copilot error"));
    }

    #[test]
    fn receipt_to_response_empty_refs() {
        let receipt = make_receipt(vec![assistant_event("ok")]);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert!(resp.copilot_references.is_empty());
    }

    // ── Streaming ──────────────────────────────────────────────────────

    #[test]
    fn stream_events_from_assistant() {
        let events = vec![assistant_event("delta")];
        let stream = events_to_stream_events(&events, "gpt-4o");
        assert!(!stream.is_empty());
    }

    #[test]
    fn stream_events_starts_with_refs() {
        let events = vec![assistant_event("hi")];
        let stream = events_to_stream_events(&events, "gpt-4o");
        assert!(matches!(
            &stream[0],
            abp_copilot_sdk::dialect::CopilotStreamEvent::CopilotReferences { .. }
        ));
    }

    // ── Client roundtrip ───────────────────────────────────────────────

    #[tokio::test]
    async fn client_roundtrip() {
        let events = vec![assistant_event("Copilot response")];
        let client = CopilotClient::new("gpt-4o").with_processor(make_processor(events));
        let req = simple_copilot_request();
        let resp = client.create(req).await.unwrap();
        assert_eq!(resp.message, "Copilot response");
    }

    #[tokio::test]
    async fn client_no_processor_error() {
        let client = CopilotClient::new("gpt-4o");
        let req = simple_copilot_request();
        let result = client.create(req).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn client_stream() {
        let events = vec![assistant_event("streamed")];
        let client = CopilotClient::new("gpt-4o").with_processor(make_processor(events));
        let req = simple_copilot_request();
        let mut stream = client.create_stream(req).await.unwrap();
        let mut count = 0;
        while let Some(_ev) = stream.next().await {
            count += 1;
        }
        assert!(count > 0);
    }

    // ── IR roundtrip ───────────────────────────────────────────────────

    #[test]
    fn messages_to_ir_roundtrip() {
        let msgs = vec![Message::user("question"), Message::assistant("answer")];
        let ir = messages_to_ir(&msgs);
        assert!(ir.messages.iter().any(|m| m.role == IrRole::User));
        assert!(ir.messages.iter().any(|m| m.role == IrRole::Assistant));
    }

    #[test]
    fn ir_to_messages_roundtrip() {
        let msgs = vec![Message::user("q")];
        let ir = messages_to_ir(&msgs);
        let back = ir_to_messages(&ir);
        assert!(!back.is_empty());
        assert_eq!(back[0].role, "user");
    }

    #[test]
    fn ir_usage_conversion() {
        let ir = IrUsage {
            input_tokens: 5,
            output_tokens: 10,
            total_tokens: 15,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        };
        let (input, output, total) = ir_usage_to_tuple(&ir);
        assert_eq!(input, 5);
        assert_eq!(output, 10);
        assert_eq!(total, 15);
    }

    // ── Builder ────────────────────────────────────────────────────────

    #[test]
    fn builder_all_fields() {
        let req = CopilotRequestBuilder::new()
            .model("gpt-4o")
            .messages(vec![Message::user("test")])
            .build();
        assert_eq!(req.model, "gpt-4o");
        assert!(!req.messages.is_empty());
    }

    // ── Message constructors ───────────────────────────────────────────

    #[test]
    fn message_system() {
        let m = Message::system("prompt");
        assert_eq!(m.role, "system");
        assert_eq!(m.content, "prompt");
    }

    #[test]
    fn message_user() {
        let m = Message::user("question");
        assert_eq!(m.role, "user");
        assert_eq!(m.content, "question");
    }

    #[test]
    fn message_assistant() {
        let m = Message::assistant("answer");
        assert_eq!(m.role, "assistant");
        assert_eq!(m.content, "answer");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  7. Cross-shim scenarios
// ═══════════════════════════════════════════════════════════════════════════

mod cross_shim {
    use super::*;

    #[test]
    fn openai_request_claude_response() {
        // OpenAI request → WorkOrder
        let oai_req = abp_shim_openai::ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![abp_shim_openai::Message::user("Cross-shim test")])
            .build();
        let wo = abp_shim_openai::request_to_work_order(&oai_req);

        // Generate a receipt as if Claude responded
        let receipt = make_receipt_with_raw_usage(
            vec![assistant_event("Claude answered this")],
            standard_usage(),
            json!({"input_tokens": 100, "output_tokens": 50}),
        );

        // Convert receipt to Claude response
        let claude_resp = abp_shim_claude::convert::from_receipt(&receipt, &wo);
        assert_eq!(claude_resp.role, "assistant");
        assert!(!claude_resp.content.is_empty());
    }

    #[test]
    fn claude_request_openai_response() {
        use abp_shim_claude::types::*;

        let claude_req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![ClaudeMessage {
                role: "user".into(),
                content: ClaudeContent::Text("Cross from Claude".into()),
            }],
            max_tokens: 1024,
            system: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stream: None,
            stop_sequences: None,
            tools: None,
            tool_choice: None,
            thinking: None,
        };
        let _wo = abp_shim_claude::convert::to_work_order(&claude_req);

        let receipt = make_receipt(vec![assistant_event("OpenAI handled this")]);
        let oai_resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(
            oai_resp.choices[0].message.content,
            Some("OpenAI handled this".into())
        );
    }

    #[test]
    fn gemini_request_codex_response() {
        use abp_shim_gemini::*;

        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("Cross Gemini→Codex")]));
        let (ir, cfg, _) = request_to_ir(&req).unwrap();
        let _wo = ir_to_work_order(&ir, "gemini-2.5-flash", &cfg);

        let receipt = make_receipt(vec![assistant_event("Codex responded")]);
        let codex_resp = abp_shim_codex::receipt_to_response(&receipt, "codex-mini-latest");
        assert!(!codex_resp.output.is_empty());
    }

    #[test]
    fn kimi_request_copilot_response() {
        let kimi_req = abp_shim_kimi::KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![abp_shim_kimi::Message::user("Cross Kimi→Copilot")])
            .build();
        let _wo = abp_shim_kimi::request_to_work_order(&kimi_req);

        let receipt = make_receipt(vec![assistant_event("Copilot answered")]);
        let copilot_resp = abp_shim_copilot::receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(copilot_resp.message, "Copilot answered");
    }

    #[test]
    fn copilot_request_kimi_response() {
        let copilot_req = abp_shim_copilot::CopilotRequestBuilder::new()
            .model("gpt-4o")
            .messages(vec![abp_shim_copilot::Message::user("Cross Copilot→Kimi")])
            .build();
        let _wo = abp_shim_copilot::request_to_work_order(&copilot_req);

        let receipt = make_receipt(vec![assistant_event("Kimi answered")]);
        let kimi_resp = abp_shim_kimi::receipt_to_response(&receipt, "moonshot-v1-8k");
        assert_eq!(
            kimi_resp.choices[0].message.content,
            Some("Kimi answered".into())
        );
    }

    #[test]
    fn codex_request_gemini_response() {
        let codex_req = abp_shim_codex::CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![abp_shim_codex::codex_message(
                "user",
                "Cross Codex→Gemini",
            )])
            .build();
        let _wo = abp_shim_codex::request_to_work_order(&codex_req);

        // Gemini produces a receipt and we convert it through Gemini's pipeline
        let receipt = make_receipt(vec![assistant_event("Gemini handled this")]);
        let ir_conv = abp_shim_gemini::receipt_to_ir(&receipt);
        assert!(!ir_conv.messages.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  8. All shims produce valid WorkOrders
// ═══════════════════════════════════════════════════════════════════════════

mod work_order_validity {
    use super::*;

    fn assert_valid_work_order(wo: &WorkOrder) {
        assert!(!wo.task.is_empty(), "task must not be empty");
        assert!(!wo.id.is_nil(), "work order id must not be nil");
    }

    #[test]
    fn openai_work_order_valid() {
        let req = abp_shim_openai::ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![abp_shim_openai::Message::user("test")])
            .build();
        let wo = abp_shim_openai::request_to_work_order(&req);
        assert_valid_work_order(&wo);
    }

    #[test]
    fn claude_work_order_valid() {
        use abp_shim_claude::types::*;
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![ClaudeMessage {
                role: "user".into(),
                content: ClaudeContent::Text("test".into()),
            }],
            max_tokens: 1024,
            system: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stream: None,
            stop_sequences: None,
            tools: None,
            tool_choice: None,
            thinking: None,
        };
        let wo = abp_shim_claude::convert::to_work_order(&req);
        assert_valid_work_order(&wo);
    }

    #[test]
    fn gemini_work_order_valid() {
        let req = abp_shim_gemini::GenerateContentRequest::new("gemini-2.5-flash").add_content(
            abp_shim_gemini::Content::user(vec![abp_shim_gemini::Part::text("test")]),
        );
        let (ir, cfg, _) = abp_shim_gemini::request_to_ir(&req).unwrap();
        let wo = abp_shim_gemini::ir_to_work_order(&ir, "gemini-2.5-flash", &cfg);
        assert_valid_work_order(&wo);
    }

    #[test]
    fn codex_work_order_valid() {
        let req = abp_shim_codex::CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![abp_shim_codex::codex_message("user", "test")])
            .build();
        let wo = abp_shim_codex::request_to_work_order(&req);
        assert_valid_work_order(&wo);
    }

    #[test]
    fn kimi_work_order_valid() {
        let req = abp_shim_kimi::KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![abp_shim_kimi::Message::user("test")])
            .build();
        let wo = abp_shim_kimi::request_to_work_order(&req);
        assert_valid_work_order(&wo);
    }

    #[test]
    fn copilot_work_order_valid() {
        let req = abp_shim_copilot::CopilotRequestBuilder::new()
            .model("gpt-4o")
            .messages(vec![abp_shim_copilot::Message::user("test")])
            .build();
        let wo = abp_shim_copilot::request_to_work_order(&req);
        assert_valid_work_order(&wo);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  9. All shims consume standard receipts
// ═══════════════════════════════════════════════════════════════════════════

mod receipt_consumption {
    use super::*;

    fn standard_receipt() -> Receipt {
        make_receipt_with_usage(
            vec![assistant_event("Standard response"), run_completed_event()],
            standard_usage(),
        )
    }

    fn receipt_with_tools() -> Receipt {
        make_receipt(vec![
            tool_call_event("search", "tc_std", json!({"q": "test"})),
            assistant_event("Found results"),
        ])
    }

    #[test]
    fn openai_consumes_standard_receipt() {
        let receipt = standard_receipt();
        let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        assert!(resp.choices[0].message.content.is_some());
        assert_eq!(resp.usage.unwrap().prompt_tokens, 100);
    }

    #[test]
    fn openai_consumes_tool_receipt() {
        let receipt = receipt_with_tools();
        let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        assert!(resp.choices[0].message.tool_calls.is_some());
    }

    #[test]
    fn claude_consumes_standard_receipt() {
        use abp_shim_claude::types::*;
        let receipt_data = standard_receipt();
        let mut receipt = receipt_data;
        receipt.usage_raw = json!({"input_tokens": 100, "output_tokens": 50});
        let wo_req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![ClaudeMessage {
                role: "user".into(),
                content: ClaudeContent::Text("test".into()),
            }],
            max_tokens: 1024,
            system: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stream: None,
            stop_sequences: None,
            tools: None,
            tool_choice: None,
            thinking: None,
        };
        let wo = abp_shim_claude::convert::to_work_order(&wo_req);
        let resp = abp_shim_claude::convert::from_receipt(&receipt, &wo);
        assert!(!resp.content.is_empty());
    }

    #[test]
    fn codex_consumes_standard_receipt() {
        let receipt = standard_receipt();
        let resp = abp_shim_codex::receipt_to_response(&receipt, "codex-mini-latest");
        assert!(!resp.output.is_empty());
    }

    #[test]
    fn codex_consumes_tool_receipt() {
        let receipt = receipt_with_tools();
        let resp = abp_shim_codex::receipt_to_response(&receipt, "codex-mini-latest");
        assert!(resp.output.iter().any(|item| matches!(
            item,
            abp_codex_sdk::dialect::CodexResponseItem::FunctionCall { .. }
        )));
    }

    #[test]
    fn kimi_consumes_standard_receipt() {
        let receipt = standard_receipt();
        let resp = abp_shim_kimi::receipt_to_response(&receipt, "moonshot-v1-8k");
        assert!(resp.choices[0].message.content.is_some());
    }

    #[test]
    fn kimi_consumes_tool_receipt() {
        let receipt = receipt_with_tools();
        let resp = abp_shim_kimi::receipt_to_response(&receipt, "moonshot-v1-8k");
        assert!(resp.choices[0].message.tool_calls.is_some());
    }

    #[test]
    fn copilot_consumes_standard_receipt() {
        let receipt = standard_receipt();
        let resp = abp_shim_copilot::receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.message, "Standard response");
    }

    #[test]
    fn copilot_consumes_tool_receipt() {
        let receipt = receipt_with_tools();
        let resp = abp_shim_copilot::receipt_to_response(&receipt, "gpt-4o");
        assert!(resp.function_call.is_some());
    }

    #[test]
    fn gemini_consumes_standard_receipt() {
        let receipt = standard_receipt();
        let ir = abp_shim_gemini::receipt_to_ir(&receipt);
        assert!(!ir.messages.is_empty());
    }

    #[test]
    fn all_shims_handle_empty_receipt() {
        let receipt = make_receipt(vec![]);

        let oai = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        assert!(oai.choices[0].message.content.is_none());

        let codex = abp_shim_codex::receipt_to_response(&receipt, "codex-mini-latest");
        assert!(codex.output.is_empty());

        let kimi = abp_shim_kimi::receipt_to_response(&receipt, "moonshot-v1-8k");
        assert!(kimi.choices[0].message.content.is_none());

        let copilot = abp_shim_copilot::receipt_to_response(&receipt, "gpt-4o");
        assert!(copilot.message.is_empty());

        let gemini_ir = abp_shim_gemini::receipt_to_ir(&receipt);
        assert!(gemini_ir.messages.is_empty());
    }

    #[test]
    fn all_shims_handle_error_receipt() {
        let receipt = make_receipt(vec![error_event("backend failure")]);

        let oai = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        assert!(
            oai.choices[0]
                .message
                .content
                .as_ref()
                .unwrap()
                .contains("Error")
        );

        let codex = abp_shim_codex::receipt_to_response(&receipt, "codex-mini-latest");
        assert!(!codex.output.is_empty());

        let kimi = abp_shim_kimi::receipt_to_response(&receipt, "moonshot-v1-8k");
        assert!(
            kimi.choices[0]
                .message
                .content
                .as_ref()
                .unwrap()
                .contains("Error")
        );

        let copilot = abp_shim_copilot::receipt_to_response(&receipt, "gpt-4o");
        assert!(!copilot.copilot_errors.is_empty());
    }
}
