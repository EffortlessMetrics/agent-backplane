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
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! Integration tests exercising the convert modules across all 6 SDK shims.
//!
//! Tests the full conversion flow: SDK request → WorkOrder → Receipt → SDK response,
//! cross-SDK translation, streaming, tool calls, system messages, and edge cases.

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, ExecutionMode, Outcome,
    Receipt, RunMetadata, UsageNormalized, VerificationReport, CONTRACT_VERSION,
};
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

// ── Shared helpers ──────────────────────────────────────────────────────

fn mock_receipt(events: Vec<AgentEvent>) -> Receipt {
    mock_receipt_with_usage(events, UsageNormalized::default())
}

fn mock_receipt_with_usage(events: Vec<AgentEvent>, usage: UsageNormalized) -> Receipt {
    let now = Utc::now();
    Receipt {
        meta: RunMetadata {
            run_id: Uuid::new_v4(),
            work_order_id: Uuid::new_v4(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: now,
            finished_at: now,
            duration_ms: 0,
        },
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::default(),
        mode: ExecutionMode::Mapped,
        usage_raw: json!({"input_tokens": 100, "output_tokens": 50}),
        usage,
        trace: events,
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

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

fn run_completed_event() -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunCompleted {
            message: "done".into(),
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

fn usage_with_tokens(input: u64, output: u64) -> UsageNormalized {
    UsageNormalized {
        input_tokens: Some(input),
        output_tokens: Some(output),
        ..Default::default()
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 1. OpenAI roundtrip
// ═══════════════════════════════════════════════════════════════════════

mod openai {
    use super::*;
    use abp_shim_openai::convert;
    use abp_shim_openai::types::*;

    fn minimal_request() -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![ChatMessage::User {
                content: MessageContent::Text("Hello".into()),
            }],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
        }
    }

    #[test]
    fn openai_roundtrip_simple() {
        let req = minimal_request();
        let wo = convert::to_work_order(&req);
        assert_eq!(wo.task, "Hello");
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));

        let receipt = mock_receipt(vec![assistant_event("Hi there!")]);
        let resp = convert::from_receipt(&receipt, &wo);
        assert_eq!(resp.object, "chat.completion");
        assert_eq!(resp.model, "gpt-4o");
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Hi there!")
        );
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn openai_roundtrip_with_params() {
        let req = ChatCompletionRequest {
            model: "gpt-4o-mini".into(),
            messages: vec![
                ChatMessage::System {
                    content: "You are helpful.".into(),
                },
                ChatMessage::User {
                    content: MessageContent::Text("Hi".into()),
                },
            ],
            temperature: Some(0.7),
            top_p: Some(0.9),
            max_tokens: Some(1024),
            stream: Some(true),
            tools: None,
            tool_choice: None,
        };
        let wo = convert::to_work_order(&req);
        assert_eq!(wo.task, "Hi");
        assert_eq!(wo.config.vendor.get("temperature").unwrap(), &json!(0.7));
        assert_eq!(wo.config.vendor.get("top_p").unwrap(), &json!(0.9));
        assert_eq!(wo.config.vendor.get("max_tokens").unwrap(), &json!(1024));
        assert_eq!(wo.config.vendor.get("stream").unwrap(), &json!(true));
    }

    #[test]
    fn openai_roundtrip_with_tools() {
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![ChatMessage::User {
                content: MessageContent::Text("What's the weather?".into()),
            }],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: Some(vec![Tool {
                tool_type: "function".into(),
                function: FunctionDef {
                    name: "get_weather".into(),
                    description: "Get weather data".into(),
                    parameters: json!({"type": "object", "properties": {"city": {"type": "string"}}}),
                },
            }]),
            tool_choice: None,
        };
        let wo = convert::to_work_order(&req);
        assert!(wo.config.vendor.contains_key("tools"));

        let receipt = mock_receipt(vec![tool_call_event(
            "get_weather",
            "call_123",
            json!({"city": "NYC"}),
        )]);
        let resp = convert::from_receipt(&receipt, &wo);
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
        let tc = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tc.len(), 1);
        assert_eq!(tc[0].function.name, "get_weather");
        assert_eq!(tc[0].id, "call_123");
    }

    #[test]
    fn openai_usage_conversion() {
        let req = minimal_request();
        let wo = convert::to_work_order(&req);
        let receipt =
            mock_receipt_with_usage(vec![assistant_event("test")], usage_with_tokens(100, 50));
        let resp = convert::from_receipt(&receipt, &wo);
        let usage = resp.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 100);
        assert_eq!(usage.completion_tokens, 50);
        assert_eq!(usage.total_tokens, 150);
    }

    #[test]
    fn openai_streaming_assistant_delta() {
        let event = delta_event("Hello ");
        let chunk = convert::from_agent_event(&event, "gpt-4o", "chunk-1");
        assert!(chunk.is_some());
        let chunk = chunk.unwrap();
        assert_eq!(chunk.choices[0].delta.content.as_deref(), Some("Hello "));
    }

    #[test]
    fn openai_streaming_tool_call() {
        let event = tool_call_event("read_file", "tc_1", json!({"path": "/tmp"}));
        let chunk = convert::from_agent_event(&event, "gpt-4o", "chunk-2");
        assert!(chunk.is_some());
        let chunk = chunk.unwrap();
        let tc = chunk.choices[0].delta.tool_calls.as_ref().unwrap();
        assert_eq!(
            tc[0].function.as_ref().unwrap().name.as_deref(),
            Some("read_file")
        );
    }

    #[test]
    fn openai_streaming_run_completed() {
        let event = run_completed_event();
        let chunk = convert::from_agent_event(&event, "gpt-4o", "chunk-3");
        assert!(chunk.is_some());
        assert_eq!(
            chunk.unwrap().choices[0].finish_reason.as_deref(),
            Some("stop")
        );
    }

    #[test]
    fn openai_streaming_ignores_irrelevant_events() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::FileChanged {
                path: "test.rs".into(),
                summary: "modified".into(),
            },
            ext: None,
        };
        assert!(convert::from_agent_event(&event, "gpt-4o", "x").is_none());
    }

    #[test]
    fn openai_error_in_receipt() {
        let req = minimal_request();
        let wo = convert::to_work_order(&req);
        let receipt = mock_receipt(vec![error_event("something went wrong")]);
        let resp = convert::from_receipt(&receipt, &wo);
        assert!(resp.choices[0]
            .message
            .content
            .as_deref()
            .unwrap()
            .contains("Error"));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Claude roundtrip
// ═══════════════════════════════════════════════════════════════════════

mod claude {
    use super::*;
    use abp_shim_claude::convert;
    use abp_shim_claude::types::*;

    fn simple_request(text: &str) -> MessagesRequest {
        MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![ClaudeMessage {
                role: "user".into(),
                content: ClaudeContent::Text(text.into()),
            }],
            max_tokens: 4096,
            system: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stream: None,
            stop_sequences: None,
            tools: None,
            tool_choice: None,
            thinking: None,
        }
    }

    #[test]
    fn claude_roundtrip_simple() {
        let req = simple_request("Hello Claude");
        let wo = convert::to_work_order(&req);
        assert_eq!(wo.task, "Hello Claude");
        assert_eq!(wo.config.model.as_deref(), Some("claude-sonnet-4-20250514"));

        let receipt = mock_receipt(vec![assistant_event("Hello human!")]);
        let resp = convert::from_receipt(&receipt, &wo);
        assert_eq!(resp.role, "assistant");
        assert_eq!(resp.type_field, "message");
        assert_eq!(resp.content.len(), 1);
        if let ContentBlock::Text { text } = &resp.content[0] {
            assert_eq!(text, "Hello human!");
        } else {
            panic!("Expected text block");
        }
    }

    #[test]
    fn claude_system_prompt_stored() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![ClaudeMessage {
                role: "user".into(),
                content: ClaudeContent::Text("Hi".into()),
            }],
            max_tokens: 1024,
            system: Some("You are a pirate.".into()),
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
        assert_eq!(
            wo.config.vendor.get("system").unwrap(),
            &json!("You are a pirate.")
        );
    }

    #[test]
    fn claude_system_prompt_fallback_task() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![],
            max_tokens: 1024,
            system: Some("System prompt as task".into()),
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
        assert_eq!(wo.task, "System prompt as task");
    }

    #[test]
    fn claude_tool_use_roundtrip() {
        let req = simple_request("Use a tool");
        let wo = convert::to_work_order(&req);

        let receipt = mock_receipt(vec![tool_call_event(
            "search",
            "tu_abc",
            json!({"query": "rust"}),
        )]);
        let resp = convert::from_receipt(&receipt, &wo);
        assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
        if let ContentBlock::ToolUse { id, name, input } = &resp.content[0] {
            assert_eq!(id, "tu_abc");
            assert_eq!(name, "search");
            assert_eq!(input, &json!({"query": "rust"}));
        } else {
            panic!("Expected ToolUse block");
        }
    }

    #[test]
    fn claude_streaming_text_delta() {
        let event = delta_event("Hello ");
        let se = convert::from_agent_event(&event);
        assert!(se.is_some());
        match se.unwrap() {
            StreamEvent::ContentBlockDelta { delta, .. } => match delta {
                StreamDelta::TextDelta { text } => assert_eq!(text, "Hello "),
                _ => panic!("Expected TextDelta"),
            },
            _ => panic!("Expected ContentBlockDelta"),
        }
    }

    #[test]
    fn claude_streaming_run_completed() {
        let event = run_completed_event();
        let se = convert::from_agent_event(&event);
        match se.unwrap() {
            StreamEvent::MessageDelta { delta, .. } => {
                assert_eq!(delta.stop_reason.as_deref(), Some("end_turn"));
            }
            _ => panic!("Expected MessageDelta"),
        }
    }

    #[test]
    fn claude_usage_from_raw() {
        let usage = convert::usage_from_raw(&json!({
            "input_tokens": 200,
            "output_tokens": 80,
            "cache_creation_input_tokens": 10
        }));
        assert_eq!(usage.input_tokens, 200);
        assert_eq!(usage.output_tokens, 80);
        assert_eq!(usage.cache_creation_input_tokens, Some(10));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Gemini roundtrip
// ═══════════════════════════════════════════════════════════════════════

mod gemini {
    use super::*;
    use abp_shim_gemini::*;

    #[test]
    fn gemini_roundtrip_simple() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("Hello Gemini")]));

        // Gemini uses an internal pipeline, so test the client's generate method
        let rt = tokio::runtime::Runtime::new().unwrap();
        let client = PipelineClient::new("gemini-2.5-flash");
        let resp = rt.block_on(client.generate(req)).unwrap();
        assert!(!resp.candidates.is_empty());
        assert!(resp.text().is_some());
    }

    #[test]
    fn gemini_function_call_in_response() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("Get weather")]))
            .tools(vec![ToolDeclaration {
                function_declarations: vec![FunctionDeclaration {
                    name: "get_weather".into(),
                    description: "Gets weather".into(),
                    parameters: json!({"type": "object", "properties": {}}),
                }],
            }]);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let client = PipelineClient::new("gemini-2.5-flash");
        let resp = rt.block_on(client.generate(req)).unwrap();
        assert!(!resp.candidates.is_empty());
    }

    #[test]
    fn gemini_generation_config_preserved() {
        let config = GenerationConfig {
            max_output_tokens: Some(512),
            temperature: Some(0.5),
            top_p: Some(0.95),
            top_k: Some(40),
            candidate_count: None,
            stop_sequences: None,
            response_mime_type: None,
            response_schema: None,
        };
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("test")]))
            .generation_config(config.clone());

        // Test dialect conversion round-trip
        let dialect_req = to_dialect_request(&req);
        assert_eq!(dialect_req.model, "gemini-2.5-flash");
        assert!(dialect_req.generation_config.is_some());
    }

    #[test]
    fn gemini_usage_ir_roundtrip() {
        let usage = UsageMetadata {
            prompt_token_count: 100,
            candidates_token_count: 50,
            total_token_count: 150,
        };
        let ir = usage_to_ir(&usage);
        let back = usage_from_ir(&ir);
        assert_eq!(back, usage);
    }

    #[test]
    fn gemini_streaming() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("Stream me")]));

        let rt = tokio::runtime::Runtime::new().unwrap();
        let client = PipelineClient::new("gemini-2.5-flash");
        let stream = rt.block_on(client.generate_stream(req)).unwrap();

        let events: Vec<_> =
            rt.block_on(async { tokio_stream::StreamExt::collect::<Vec<_>>(stream).await });
        assert!(!events.is_empty());
    }

    #[test]
    fn gemini_system_instruction() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("Hi")]))
            .system_instruction(Content {
                role: "user".into(),
                parts: vec![Part::text("Be concise")],
            });
        let dialect = to_dialect_request(&req);
        assert!(dialect.system_instruction.is_some());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Codex roundtrip
// ═══════════════════════════════════════════════════════════════════════

mod codex {
    use super::*;
    use abp_shim_codex::{
        codex_message, events_to_stream_events, ir_usage_to_usage, receipt_to_response,
        request_to_ir, request_to_work_order, CodexRequestBuilder,
    };

    #[test]
    fn codex_roundtrip_simple() {
        let req = CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![codex_message("user", "Fix the bug")])
            .build();

        let wo = request_to_work_order(&req);
        assert_eq!(wo.task, "Fix the bug");
        assert_eq!(wo.config.model.as_deref(), Some("codex-mini-latest"));

        let receipt = mock_receipt(vec![assistant_event("Bug fixed.")]);
        let resp = receipt_to_response(&receipt, "codex-mini-latest");
        assert!(resp.status.as_deref() == Some("completed"));
        assert!(!resp.output.is_empty());
    }

    #[test]
    fn codex_with_temperature() {
        let req = CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![codex_message("user", "test")])
            .temperature(0.3)
            .max_output_tokens(2048)
            .build();

        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.vendor.get("temperature").unwrap(), &json!(0.3));
        assert_eq!(
            wo.config.vendor.get("max_output_tokens").unwrap(),
            &json!(2048)
        );
    }

    #[test]
    fn codex_tool_call_in_response() {
        let receipt = mock_receipt(vec![tool_call_event(
            "execute_command",
            "fc_001",
            json!({"command": "cargo test"}),
        )]);
        let resp = receipt_to_response(&receipt, "codex-mini-latest");
        let has_fn_call = resp.output.iter().any(|item| {
            matches!(item, abp_codex_sdk::dialect::CodexResponseItem::FunctionCall { name, .. } if name == "execute_command")
        });
        assert!(has_fn_call);
    }

    #[test]
    fn codex_streaming_events() {
        let events = vec![delta_event("Hello "), assistant_event("World")];
        let stream = events_to_stream_events(&events, "codex-mini-latest");
        // Should have: ResponseCreated, delta, message done, ResponseCompleted
        assert!(stream.len() >= 3);
    }

    #[test]
    fn codex_error_in_receipt() {
        let receipt = mock_receipt(vec![error_event("compilation failed")]);
        let resp = receipt_to_response(&receipt, "codex-mini-latest");
        let has_error = resp.output.iter().any(|item| {
            matches!(item, abp_codex_sdk::dialect::CodexResponseItem::Message { content, .. }
                if content.iter().any(|c| matches!(c, abp_codex_sdk::dialect::CodexContentPart::OutputText { text } if text.contains("Error"))))
        });
        assert!(has_error);
    }

    #[test]
    fn codex_ir_roundtrip() {
        let req = CodexRequestBuilder::new()
            .input(vec![codex_message("user", "Hello")])
            .build();
        let ir = request_to_ir(&req);
        assert!(!ir.messages.is_empty());
    }

    #[test]
    fn codex_usage_conversion() {
        let ir = abp_core::ir::IrUsage::from_io(100, 50);
        let usage = ir_usage_to_usage(&ir);
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.total_tokens, 150);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Kimi roundtrip
// ═══════════════════════════════════════════════════════════════════════

mod kimi {
    use super::*;
    use abp_shim_kimi::{
        events_to_stream_chunks, ir_to_messages, ir_usage_to_usage, messages_to_ir,
        receipt_to_response, request_to_work_order, KimiRequestBuilder, Message,
    };

    #[test]
    fn kimi_roundtrip_simple() {
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![Message::user("Hello Kimi")])
            .build();

        let wo = request_to_work_order(&req);
        assert_eq!(wo.task, "Hello Kimi");
        assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-8k"));

        let receipt = mock_receipt(vec![assistant_event("Hello!")]);
        let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
        assert_eq!(resp.model, "moonshot-v1-8k");
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hello!"));
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn kimi_with_search_option() {
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![Message::user("search test")])
            .use_search(true)
            .build();

        assert!(req.use_search == Some(true));
        let wo = request_to_work_order(&req);
        assert_eq!(wo.task, "search test");
    }

    #[test]
    fn kimi_tool_call_roundtrip() {
        let receipt = mock_receipt(vec![tool_call_event(
            "web_search",
            "call_999",
            json!({"query": "Rust lang"}),
        )]);
        let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
        let tc = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tc[0].function.name, "web_search");
        assert_eq!(tc[0].id, "call_999");
    }

    #[test]
    fn kimi_streaming_chunks() {
        let events = vec![delta_event("chunk1"), assistant_event("chunk2")];
        let chunks = events_to_stream_chunks(&events, "moonshot-v1-8k");
        // delta + message + final stop chunk
        assert!(chunks.len() >= 3);
        assert_eq!(
            chunks.last().unwrap().choices[0].finish_reason.as_deref(),
            Some("stop")
        );
    }

    #[test]
    fn kimi_ir_roundtrip() {
        let messages = vec![
            Message::system("Be helpful"),
            Message::user("Hello"),
            Message::assistant("Hi"),
        ];
        let ir = messages_to_ir(&messages);
        let back = ir_to_messages(&ir);
        assert!(!back.is_empty());
    }

    #[test]
    fn kimi_usage_conversion() {
        let ir = abp_core::ir::IrUsage::from_io(200, 100);
        let usage = ir_usage_to_usage(&ir);
        assert_eq!(usage.prompt_tokens, 200);
        assert_eq!(usage.completion_tokens, 100);
        assert_eq!(usage.total_tokens, 300);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Copilot roundtrip
// ═══════════════════════════════════════════════════════════════════════

mod copilot {
    use super::*;
    use abp_copilot_sdk::dialect::{CopilotReference, CopilotReferenceType};
    use abp_shim_copilot::{
        events_to_stream_events, ir_to_messages, ir_usage_to_tuple, messages_to_ir,
        receipt_to_response, request_to_work_order, CopilotRequestBuilder, Message,
    };

    #[test]
    fn copilot_roundtrip_simple() {
        let req = CopilotRequestBuilder::new()
            .model("gpt-4o")
            .messages(vec![Message::user("Hello Copilot")])
            .build();

        let wo = request_to_work_order(&req);
        assert_eq!(wo.task, "Hello Copilot");
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));

        let receipt = mock_receipt(vec![assistant_event("Sure, I can help!")]);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.message, "Sure, I can help!");
        assert!(resp.copilot_errors.is_empty());
    }

    #[test]
    fn copilot_with_references() {
        let refs = vec![CopilotReference {
            ref_type: CopilotReferenceType::File,
            id: "file_1".into(),
            data: json!({"path": "src/main.rs", "content": "fn main() {}"}),
            metadata: None,
        }];
        let req = CopilotRequestBuilder::new()
            .model("gpt-4o")
            .messages(vec![Message::user_with_refs("Check this file", refs)])
            .build();

        let wo = request_to_work_order(&req);
        assert_eq!(wo.task, "Check this file");
    }

    #[test]
    fn copilot_function_call_in_response() {
        let receipt = mock_receipt(vec![tool_call_event(
            "run_tests",
            "fc_cop_1",
            json!({"suite": "unit"}),
        )]);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert!(resp.function_call.is_some());
        let fc = resp.function_call.unwrap();
        assert_eq!(fc.name, "run_tests");
    }

    #[test]
    fn copilot_streaming_events() {
        let events = vec![delta_event("Hello "), assistant_event("World")];
        let stream = events_to_stream_events(&events, "gpt-4o");
        // CopilotReferences + TextDelta + TextDelta + Done
        assert!(stream.len() >= 3);
    }

    #[test]
    fn copilot_error_in_response() {
        let receipt = mock_receipt(vec![error_event("rate limited")]);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert!(!resp.copilot_errors.is_empty());
        assert!(resp.copilot_errors[0].message.contains("rate limited"));
    }

    #[test]
    fn copilot_ir_roundtrip() {
        let messages = vec![
            Message::system("System"),
            Message::user("User msg"),
            Message::assistant("Assistant msg"),
        ];
        let ir = messages_to_ir(&messages);
        let back = ir_to_messages(&ir);
        assert!(!back.is_empty());
    }

    #[test]
    fn copilot_usage_tuple() {
        let ir = abp_core::ir::IrUsage::from_io(300, 150);
        let (input, output, total) = ir_usage_to_tuple(&ir);
        assert_eq!(input, 300);
        assert_eq!(output, 150);
        assert_eq!(total, 450);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Cross-SDK: OpenAI → WorkOrder → Claude response
// ═══════════════════════════════════════════════════════════════════════

mod cross_sdk {
    use super::*;
    use abp_shim_claude::convert as claude_convert;
    use abp_shim_claude::types::*;
    use abp_shim_openai::convert as openai_convert;
    use abp_shim_openai::types::*;

    #[test]
    fn openai_to_workorder_to_claude_response() {
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![ChatMessage::User {
                content: MessageContent::Text("Translate me".into()),
            }],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
        };
        let wo = openai_convert::to_work_order(&req);
        assert_eq!(wo.task, "Translate me");

        let receipt = mock_receipt(vec![assistant_event("Translated!")]);
        let claude_resp = claude_convert::from_receipt(&receipt, &wo);
        assert_eq!(claude_resp.role, "assistant");
        assert_eq!(claude_resp.content.len(), 1);
        if let ContentBlock::Text { text } = &claude_resp.content[0] {
            assert_eq!(text, "Translated!");
        } else {
            panic!("Expected text block");
        }
    }

    #[test]
    fn claude_to_workorder_to_openai_response() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![ClaudeMessage {
                role: "user".into(),
                content: ClaudeContent::Text("Cross SDK test".into()),
            }],
            max_tokens: 4096,
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
        let wo = claude_convert::to_work_order(&req);
        assert_eq!(wo.task, "Cross SDK test");

        let receipt = mock_receipt(vec![assistant_event("Response from ABP")]);
        let openai_resp = openai_convert::from_receipt(&receipt, &wo);
        assert_eq!(openai_resp.object, "chat.completion");
        assert_eq!(
            openai_resp.choices[0].message.content.as_deref(),
            Some("Response from ABP")
        );
    }

    #[test]
    fn cross_sdk_tool_call_survives() {
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![ChatMessage::User {
                content: MessageContent::Text("Use tool".into()),
            }],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
        };
        let wo = openai_convert::to_work_order(&req);

        let receipt = mock_receipt(vec![tool_call_event(
            "search",
            "cross_tc_1",
            json!({"q": "test"}),
        )]);

        // Same receipt, but interpreted by Claude
        let claude_resp = claude_convert::from_receipt(&receipt, &wo);
        assert!(claude_resp.content.iter().any(|b| matches!(b,
            ContentBlock::ToolUse { name, .. } if name == "search"
        )));

        // And by OpenAI
        let openai_resp = openai_convert::from_receipt(&receipt, &wo);
        let tc = openai_resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tc[0].function.name, "search");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Streaming: from_agent_event across SDKs
// ═══════════════════════════════════════════════════════════════════════

mod streaming {
    use super::*;

    #[test]
    fn openai_produces_stream_chunk() {
        use abp_shim_openai::convert;
        let event = delta_event("streamed");
        let chunk = convert::from_agent_event(&event, "gpt-4o", "s1");
        assert!(chunk.is_some());
        assert_eq!(chunk.unwrap().object, "chat.completion.chunk");
    }

    #[test]
    fn claude_produces_stream_event() {
        use abp_shim_claude::convert;
        let event = delta_event("streamed");
        let se = convert::from_agent_event(&event);
        assert!(se.is_some());
    }

    #[test]
    fn codex_produces_stream_events() {
        use abp_shim_codex::events_to_stream_events;
        let events = vec![delta_event("one"), delta_event("two")];
        let stream = events_to_stream_events(&events, "codex-mini-latest");
        assert!(stream.len() >= 4); // created + 2 deltas + completed
    }

    #[test]
    fn kimi_produces_stream_chunks() {
        use abp_shim_kimi::events_to_stream_chunks;
        let events = vec![delta_event("hello"), assistant_event("world")];
        let chunks = events_to_stream_chunks(&events, "moonshot-v1-8k");
        assert!(chunks.len() >= 3);
    }

    #[test]
    fn copilot_produces_stream_events() {
        use abp_shim_copilot::events_to_stream_events;
        let events = vec![delta_event("test")];
        let stream = events_to_stream_events(&events, "gpt-4o");
        // references + delta + done
        assert!(stream.len() >= 3);
    }

    #[test]
    fn gemini_streaming_produces_events() {
        use abp_shim_gemini::*;
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("stream")]));

        let rt = tokio::runtime::Runtime::new().unwrap();
        let client = PipelineClient::new("gemini-2.5-flash");
        let stream = rt.block_on(client.generate_stream(req)).unwrap();

        let events: Vec<_> =
            rt.block_on(async { tokio_stream::StreamExt::collect::<Vec<_>>(stream).await });
        assert!(!events.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Tool calls survive conversion
// ═══════════════════════════════════════════════════════════════════════

mod tool_calls {
    use super::*;

    #[test]
    fn openai_tool_definitions_stored_in_vendor() {
        use abp_shim_openai::convert;
        use abp_shim_openai::types::*;

        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![ChatMessage::User {
                content: MessageContent::Text("test".into()),
            }],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: Some(vec![
                Tool {
                    tool_type: "function".into(),
                    function: FunctionDef {
                        name: "tool_a".into(),
                        description: "Does A".into(),
                        parameters: json!({"type": "object"}),
                    },
                },
                Tool {
                    tool_type: "function".into(),
                    function: FunctionDef {
                        name: "tool_b".into(),
                        description: "Does B".into(),
                        parameters: json!({"type": "object"}),
                    },
                },
            ]),
            tool_choice: None,
        };
        let wo = convert::to_work_order(&req);
        let tools = wo.config.vendor.get("tools").unwrap();
        let arr = tools.as_array().unwrap();
        assert_eq!(arr.len(), 2);
    }

    #[test]
    fn claude_tool_definitions_stored_in_vendor() {
        use abp_shim_claude::convert;
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
            tools: Some(vec![ClaudeTool {
                name: "calculator".into(),
                description: Some("Math".into()),
                input_schema: json!({"type": "object"}),
            }]),
            tool_choice: None,
            thinking: None,
        };
        let wo = convert::to_work_order(&req);
        assert!(wo.config.vendor.contains_key("tools"));
    }

    #[test]
    fn tool_call_event_roundtrips_through_all_response_formats() {
        let tc_event = tool_call_event("my_tool", "tc_round", json!({"key": "value"}));
        let receipt = mock_receipt(vec![tc_event]);

        // OpenAI
        {
            use abp_shim_openai::convert;
            let wo = abp_core::WorkOrderBuilder::new("test")
                .model("gpt-4o")
                .build();
            let resp = convert::from_receipt(&receipt, &wo);
            let tc = resp.choices[0].message.tool_calls.as_ref().unwrap();
            assert_eq!(tc[0].function.name, "my_tool");
            assert_eq!(tc[0].id, "tc_round");
        }

        // Claude
        {
            use abp_shim_claude::convert;
            let wo = abp_core::WorkOrderBuilder::new("test")
                .model("claude-sonnet-4-20250514")
                .build();
            let resp = convert::from_receipt(&receipt, &wo);
            let tu = &resp.content[0];
            if let abp_shim_claude::types::ContentBlock::ToolUse { id, name, .. } = tu {
                assert_eq!(name, "my_tool");
                assert_eq!(id, "tc_round");
            } else {
                panic!("Expected ToolUse");
            }
        }

        // Kimi
        {
            let resp = abp_shim_kimi::receipt_to_response(&receipt, "moonshot-v1-8k");
            let tc = resp.choices[0].message.tool_calls.as_ref().unwrap();
            assert_eq!(tc[0].function.name, "my_tool");
        }

        // Codex
        {
            let resp = abp_shim_codex::receipt_to_response(&receipt, "codex-mini-latest");
            let has_fc = resp.output.iter().any(|item| {
                matches!(item, abp_codex_sdk::dialect::CodexResponseItem::FunctionCall { name, .. } if name == "my_tool")
            });
            assert!(has_fc);
        }

        // Copilot
        {
            let resp = abp_shim_copilot::receipt_to_response(&receipt, "gpt-4o");
            let fc = resp.function_call.unwrap();
            assert_eq!(fc.name, "my_tool");
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 10. System messages
// ═══════════════════════════════════════════════════════════════════════

mod system_messages {
    use super::*;

    #[test]
    fn claude_extracts_system_prompt() {
        use abp_shim_claude::convert;
        use abp_shim_claude::types::*;

        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![ClaudeMessage {
                role: "user".into(),
                content: ClaudeContent::Text("Hi".into()),
            }],
            max_tokens: 1024,
            system: Some("You are an expert.".into()),
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
        assert_eq!(
            wo.config.vendor.get("system").unwrap(),
            &json!("You are an expert.")
        );
    }

    #[test]
    fn openai_system_message_in_request() {
        use abp_shim_openai::convert;
        use abp_shim_openai::types::*;

        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![
                ChatMessage::System {
                    content: "Be brief.".into(),
                },
                ChatMessage::User {
                    content: MessageContent::Text("Hello".into()),
                },
            ],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
        };
        let wo = convert::to_work_order(&req);
        // Task should come from the user message, not system
        assert_eq!(wo.task, "Hello");
    }

    #[test]
    fn kimi_system_message_ir_roundtrip() {
        use abp_shim_kimi::*;

        let messages = vec![Message::system("You are a pirate"), Message::user("Hello")];
        let ir = messages_to_ir(&messages);
        let back = ir_to_messages(&ir);
        // System message should survive IR roundtrip
        assert!(back.iter().any(|m| m.role == "system"));
    }

    #[test]
    fn copilot_system_message_ir_roundtrip() {
        use abp_shim_copilot::*;

        let messages = vec![
            Message::system("System prompt"),
            Message::user("User input"),
        ];
        let ir = messages_to_ir(&messages);
        let back = ir_to_messages(&ir);
        assert!(back.iter().any(|m| m.role == "system"));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Edge cases
// ═══════════════════════════════════════════════════════════════════════

mod edge_cases {
    use super::*;

    #[test]
    fn openai_empty_messages() {
        use abp_shim_openai::convert;
        use abp_shim_openai::types::*;

        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
        };
        let wo = convert::to_work_order(&req);
        assert_eq!(wo.task, "chat completion");
    }

    #[test]
    fn claude_empty_messages() {
        use abp_shim_claude::convert;
        use abp_shim_claude::types::*;

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
    fn empty_receipt_trace() {
        use abp_shim_openai::convert as openai_convert;
        use abp_shim_openai::types::*;

        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![ChatMessage::User {
                content: MessageContent::Text("test".into()),
            }],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
        };
        let wo = openai_convert::to_work_order(&req);
        let receipt = mock_receipt(vec![]);
        let resp = openai_convert::from_receipt(&receipt, &wo);
        assert!(resp.choices[0].message.content.is_none());
    }

    #[test]
    fn codex_empty_input() {
        use abp_shim_codex::*;

        let req = CodexRequestBuilder::new().build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.task, "codex completion");
        assert_eq!(wo.config.model.as_deref(), Some("codex-mini-latest"));
    }

    #[test]
    fn kimi_empty_messages() {
        use abp_shim_kimi::*;

        let req = KimiRequestBuilder::new().messages(vec![]).build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.task, "kimi completion");
    }

    #[test]
    fn copilot_empty_messages() {
        use abp_shim_copilot::*;

        let req = CopilotRequestBuilder::new().messages(vec![]).build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.task, "copilot completion");
    }

    #[test]
    fn no_tools_in_request() {
        use abp_shim_openai::convert;
        use abp_shim_openai::types::*;

        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![ChatMessage::User {
                content: MessageContent::Text("no tools".into()),
            }],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
        };
        let wo = convert::to_work_order(&req);
        assert!(!wo.config.vendor.contains_key("tools"));
    }

    #[test]
    fn receipt_with_multiple_events() {
        use abp_shim_openai::convert;
        use abp_shim_openai::types::*;

        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![ChatMessage::User {
                content: MessageContent::Text("multi".into()),
            }],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
        };
        let wo = convert::to_work_order(&req);
        let receipt = mock_receipt(vec![
            assistant_event("Part 1"),
            tool_call_event("fn_x", "tc_x", json!({})),
        ]);
        let resp = convert::from_receipt(&receipt, &wo);
        // Should have content from AssistantMessage and tool calls
        assert!(resp.choices[0].message.content.is_some());
        assert!(resp.choices[0].message.tool_calls.is_some());
    }

    #[test]
    fn claude_content_blocks_variant() {
        use abp_shim_claude::convert;
        use abp_shim_claude::types::*;

        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![ClaudeMessage {
                role: "user".into(),
                content: ClaudeContent::Blocks(vec![
                    ContentBlock::Text {
                        text: "Block 1".into(),
                    },
                    ContentBlock::Text {
                        text: " Block 2".into(),
                    },
                ]),
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
        assert_eq!(wo.task, "Block 1 Block 2");
    }

    #[test]
    fn openai_delta_accumulation_in_receipt() {
        use abp_shim_openai::convert;
        use abp_shim_openai::types::*;

        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![ChatMessage::User {
                content: MessageContent::Text("test".into()),
            }],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
        };
        let wo = convert::to_work_order(&req);
        let receipt = mock_receipt(vec![delta_event("Hello "), delta_event("World")]);
        let resp = convert::from_receipt(&receipt, &wo);
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Hello World")
        );
    }

    #[test]
    fn gemini_empty_contents() {
        use abp_shim_gemini::*;

        let req = GenerateContentRequest::new("gemini-2.5-flash");
        let dialect = to_dialect_request(&req);
        assert!(dialect.contents.is_empty());
    }
}
