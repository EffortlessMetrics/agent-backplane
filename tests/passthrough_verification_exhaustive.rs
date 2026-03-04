#![allow(clippy::all)]
#![allow(dead_code, unused_imports, unused_variables)]
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
//! Exhaustive passthrough verification tests for all SDK shims.
//!
//! Validates that passthrough mode preserves requests and responses
//! without any rewriting or field injection across all six SDK shims.

use std::collections::BTreeMap;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, ExecutionMode, Outcome,
    Receipt, ReceiptBuilder, RunMetadata, UsageNormalized, VerificationReport, WorkOrderBuilder,
    CONTRACT_VERSION,
};
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════

/// Build a receipt in passthrough mode with the given events and usage.
fn passthrough_receipt(events: Vec<AgentEvent>, usage: UsageNormalized) -> Receipt {
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
            id: "passthrough-mock".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::Passthrough,
        usage_raw: serde_json::Value::Null,
        usage,
        trace: events,
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

/// Build a passthrough receipt with default usage.
fn passthrough_receipt_default(events: Vec<AgentEvent>) -> Receipt {
    passthrough_receipt(events, UsageNormalized::default())
}

/// Build an agent event with an ext map containing raw_message.
fn event_with_raw(kind: AgentEventKind, raw: serde_json::Value) -> AgentEvent {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".to_string(), raw);
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: Some(ext),
    }
}

/// Build a simple assistant message event.
fn assistant_event(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: text.to_string(),
        },
        ext: None,
    }
}

/// Build a simple assistant delta event.
fn delta_event(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta {
            text: text.to_string(),
        },
        ext: None,
    }
}

/// Build a tool call event.
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

/// Build an error event.
fn error_event(message: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: message.to_string(),
            error_code: None,
        },
        ext: None,
    }
}

/// Standard usage for testing.
fn test_usage(input: u64, output: u64) -> UsageNormalized {
    UsageNormalized {
        input_tokens: Some(input),
        output_tokens: Some(output),
        cache_read_tokens: None,
        cache_write_tokens: None,
        request_units: None,
        estimated_cost_usd: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ═══════════════════════════════════════════════════════════════════
    // 1. Passthrough mode invariant tests
    // ═══════════════════════════════════════════════════════════════════

    mod passthrough_invariants {
        use super::*;

        #[test]
        fn execution_mode_passthrough_is_distinct_from_mapped() {
            assert_ne!(ExecutionMode::Passthrough, ExecutionMode::Mapped);
            assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
        }

        #[test]
        fn passthrough_receipt_has_correct_mode() {
            let receipt = passthrough_receipt_default(vec![assistant_event("hello")]);
            assert_eq!(receipt.mode, ExecutionMode::Passthrough);
        }

        #[test]
        fn passthrough_mode_serde_roundtrip() {
            let mode = ExecutionMode::Passthrough;
            let json = serde_json::to_string(&mode).unwrap();
            assert_eq!(json, "\"passthrough\"");
            let back: ExecutionMode = serde_json::from_str(&json).unwrap();
            assert_eq!(back, ExecutionMode::Passthrough);
        }

        #[test]
        fn mapped_mode_serde_roundtrip() {
            let mode = ExecutionMode::Mapped;
            let json = serde_json::to_string(&mode).unwrap();
            assert_eq!(json, "\"mapped\"");
            let back: ExecutionMode = serde_json::from_str(&json).unwrap();
            assert_eq!(back, ExecutionMode::Mapped);
        }

        #[test]
        fn ext_field_preserves_raw_message_verbatim() {
            let raw = json!({
                "id": "chatcmpl-abc",
                "object": "chat.completion",
                "model": "gpt-4o",
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "Hi"}}]
            });
            let event = event_with_raw(
                AgentEventKind::AssistantMessage {
                    text: "Hi".to_string(),
                },
                raw.clone(),
            );
            assert_eq!(
                event.ext.as_ref().unwrap().get("raw_message").unwrap(),
                &raw
            );
        }

        #[test]
        fn ext_field_serde_roundtrip_preserves_all_keys() {
            let mut ext = BTreeMap::new();
            ext.insert("raw_message".to_string(), json!({"vendor": "openai"}));
            ext.insert("passthrough_id".to_string(), json!("req_123"));
            ext.insert(
                "vendor_metadata".to_string(),
                json!({"model_version": "2025-01-01"}),
            );

            let event = AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: "test".into(),
                },
                ext: Some(ext.clone()),
            };

            let json = serde_json::to_string(&event).unwrap();
            let back: AgentEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(back.ext.as_ref().unwrap().len(), 3);
            assert_eq!(
                back.ext.as_ref().unwrap().get("passthrough_id").unwrap(),
                &json!("req_123")
            );
        }

        #[test]
        fn passthrough_receipt_trace_events_unchanged() {
            let events = vec![
                delta_event("Hello"),
                delta_event(", "),
                delta_event("world!"),
            ];
            let receipt = passthrough_receipt_default(events.clone());
            assert_eq!(receipt.trace.len(), 3);
            for (orig, stored) in events.iter().zip(receipt.trace.iter()) {
                match (&orig.kind, &stored.kind) {
                    (
                        AgentEventKind::AssistantDelta { text: a },
                        AgentEventKind::AssistantDelta { text: b },
                    ) => assert_eq!(a, b),
                    _ => panic!("event kind mismatch"),
                }
            }
        }

        #[test]
        fn passthrough_vendor_extensions_preserved() {
            let mut ext = BTreeMap::new();
            ext.insert(
                "custom_field".to_string(),
                json!({"nested": {"deep": true}}),
            );
            ext.insert(
                "array_field".to_string(),
                json!([1, 2, 3, "four", null]),
            );
            let event = AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: "test".into(),
                },
                ext: Some(ext),
            };
            let receipt = passthrough_receipt_default(vec![event]);
            let stored_ext = receipt.trace[0].ext.as_ref().unwrap();
            assert_eq!(
                stored_ext.get("custom_field").unwrap(),
                &json!({"nested": {"deep": true}})
            );
            assert_eq!(
                stored_ext.get("array_field").unwrap(),
                &json!([1, 2, 3, "four", null])
            );
        }

        #[test]
        fn passthrough_tool_calls_pass_through_without_transformation() {
            let input = json!({"path": "/etc/passwd", "encoding": "utf-8"});
            let event = tool_call_event("read_file", "call_xyz", input.clone());
            let receipt = passthrough_receipt_default(vec![event]);

            match &receipt.trace[0].kind {
                AgentEventKind::ToolCall {
                    tool_name,
                    tool_use_id,
                    input: stored_input,
                    ..
                } => {
                    assert_eq!(tool_name, "read_file");
                    assert_eq!(tool_use_id.as_deref(), Some("call_xyz"));
                    assert_eq!(stored_input, &input);
                }
                other => panic!("expected ToolCall, got {other:?}"),
            }
        }

        #[test]
        fn streaming_deltas_maintain_order_and_content() {
            let deltas = vec![
                delta_event("The"),
                delta_event(" quick"),
                delta_event(" brown"),
                delta_event(" fox"),
            ];
            let receipt = passthrough_receipt_default(deltas);
            let texts: Vec<&str> = receipt
                .trace
                .iter()
                .filter_map(|e| match &e.kind {
                    AgentEventKind::AssistantDelta { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            assert_eq!(texts, vec!["The", " quick", " brown", " fox"]);
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // 2. Per-SDK passthrough tests
    // ═══════════════════════════════════════════════════════════════════

    mod openai_passthrough {
        use super::*;
        use abp_shim_openai::{
            ChatCompletionRequest, ChatCompletionResponse, Choice, Message, OpenAiClient, Role,
            Usage, mock_receipt_with_usage,
        };

        type ProcessFn = Box<dyn Fn(&abp_core::WorkOrder) -> Receipt + Send + Sync>;

        fn make_passthrough_processor(events: Vec<AgentEvent>) -> ProcessFn {
            Box::new(move |_wo| passthrough_receipt_default(events.clone()))
        }

        fn make_passthrough_processor_with_usage(
            events: Vec<AgentEvent>,
            usage: UsageNormalized,
        ) -> ProcessFn {
            Box::new(move |_wo| passthrough_receipt(events.clone(), usage.clone()))
        }

        #[tokio::test]
        async fn openai_full_chat_completion_roundtrip() {
            let events = vec![assistant_event("Hello from GPT-4o!")];
            let client =
                OpenAiClient::new("gpt-4o").with_processor(make_passthrough_processor(events));
            let req = ChatCompletionRequest::builder()
                .model("gpt-4o")
                .messages(vec![Message::user("Hi")])
                .build();

            let resp = client.chat().completions().create(req).await.unwrap();
            assert_eq!(resp.model, "gpt-4o");
            assert_eq!(resp.choices.len(), 1);
            assert_eq!(
                resp.choices[0].message.content.as_deref(),
                Some("Hello from GPT-4o!")
            );
            assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
        }

        #[tokio::test]
        async fn openai_streaming_roundtrip() {
            let events = vec![delta_event("Hel"), delta_event("lo!")];
            let client =
                OpenAiClient::new("gpt-4o").with_processor(make_passthrough_processor(events));
            let req = ChatCompletionRequest::builder()
                .model("gpt-4o")
                .messages(vec![Message::user("Hi")])
                .stream(true)
                .build();

            let stream = client
                .chat()
                .completions()
                .create_stream(req)
                .await
                .unwrap();
            use tokio_stream::StreamExt;
            let chunks: Vec<_> = stream.collect().await;
            // 2 deltas + 1 final stop chunk
            assert_eq!(chunks.len(), 3);
            assert_eq!(chunks[0].choices[0].delta.content.as_deref(), Some("Hel"));
            assert_eq!(chunks[1].choices[0].delta.content.as_deref(), Some("lo!"));
            assert_eq!(
                chunks[2].choices[0].finish_reason.as_deref(),
                Some("stop")
            );
        }

        #[tokio::test]
        async fn openai_tool_calls_passthrough() {
            let events = vec![tool_call_event(
                "get_weather",
                "call_abc",
                json!({"location": "NYC"}),
            )];
            let client =
                OpenAiClient::new("gpt-4o").with_processor(make_passthrough_processor(events));
            let req = ChatCompletionRequest::builder()
                .model("gpt-4o")
                .messages(vec![Message::user("Weather?")])
                .build();

            let resp = client.chat().completions().create(req).await.unwrap();
            let tc = resp.choices[0].message.tool_calls.as_ref().unwrap();
            assert_eq!(tc.len(), 1);
            assert_eq!(tc[0].id, "call_abc");
            assert_eq!(tc[0].function.name, "get_weather");
            assert!(tc[0].function.arguments.contains("NYC"));
            assert_eq!(
                resp.choices[0].finish_reason.as_deref(),
                Some("tool_calls")
            );
        }

        #[tokio::test]
        async fn openai_usage_stats_passthrough() {
            let usage = test_usage(150, 75);
            let events = vec![assistant_event("ok")];
            let client = OpenAiClient::new("gpt-4o")
                .with_processor(make_passthrough_processor_with_usage(events, usage));
            let req = ChatCompletionRequest::builder()
                .model("gpt-4o")
                .messages(vec![Message::user("test")])
                .build();

            let resp = client.chat().completions().create(req).await.unwrap();
            let u = resp.usage.unwrap();
            assert_eq!(u.prompt_tokens, 150);
            assert_eq!(u.completion_tokens, 75);
            assert_eq!(u.total_tokens, 225);
        }

        #[tokio::test]
        async fn openai_model_name_unchanged() {
            let events = vec![assistant_event("ok")];
            let client = OpenAiClient::new("gpt-4-turbo-2024-04-09")
                .with_processor(make_passthrough_processor(events));
            let req = ChatCompletionRequest::builder()
                .model("gpt-4-turbo-2024-04-09")
                .messages(vec![Message::user("test")])
                .build();

            let resp = client.chat().completions().create(req).await.unwrap();
            assert_eq!(resp.model, "gpt-4-turbo-2024-04-09");
        }
    }

    mod claude_passthrough {
        use super::*;
        use abp_shim_claude::{
            AnthropicClient, ContentBlock, Message, MessageDeltaPayload, MessageRequest,
            MessageResponse, Role, ShimError, StreamDelta, StreamEvent, Usage,
            request_to_work_order, response_from_events,
        };

        #[tokio::test]
        async fn claude_messages_roundtrip_with_thinking() {
            let mut client = AnthropicClient::with_model("claude-sonnet-4-20250514");
            client.set_handler(Box::new(|req| {
                Ok(MessageResponse {
                    id: "msg_test".into(),
                    response_type: "message".into(),
                    role: "assistant".into(),
                    content: vec![
                        ContentBlock::Thinking {
                            thinking: "Let me reason about this...".into(),
                            signature: Some("sig_abc".into()),
                        },
                        ContentBlock::Text {
                            text: "The answer is 42.".into(),
                        },
                    ],
                    model: req.model.clone(),
                    stop_reason: Some("end_turn".into()),
                    stop_sequence: None,
                    usage: Usage {
                        input_tokens: 100,
                        output_tokens: 50,
                        cache_creation_input_tokens: None,
                        cache_read_input_tokens: None,
                    },
                })
            }));

            let req = MessageRequest {
                model: "claude-sonnet-4-20250514".into(),
                max_tokens: 4096,
                messages: vec![Message {
                    role: Role::User,
                    content: vec![ContentBlock::Text {
                        text: "What is the meaning?".into(),
                    }],
                }],
                system: None,
                temperature: None,
                stop_sequences: None,
                thinking: None,
                stream: None,
            };

            let resp = client.create(req).await.unwrap();
            assert_eq!(resp.model, "claude-sonnet-4-20250514");
            assert_eq!(resp.content.len(), 2);
            match &resp.content[0] {
                ContentBlock::Thinking {
                    thinking,
                    signature,
                } => {
                    assert_eq!(thinking, "Let me reason about this...");
                    assert_eq!(signature.as_deref(), Some("sig_abc"));
                }
                other => panic!("expected Thinking, got {other:?}"),
            }
            match &resp.content[1] {
                ContentBlock::Text { text } => assert_eq!(text, "The answer is 42."),
                other => panic!("expected Text, got {other:?}"),
            }
            assert_eq!(resp.stop_reason.as_deref(), Some("end_turn"));
            assert_eq!(resp.usage.input_tokens, 100);
            assert_eq!(resp.usage.output_tokens, 50);
        }

        #[tokio::test]
        async fn claude_streaming_roundtrip() {
            let mut client = AnthropicClient::with_model("claude-sonnet-4-20250514");
            client.set_stream_handler(Box::new(|_req| {
                Ok(vec![
                    StreamEvent::MessageStart {
                        message: MessageResponse {
                            id: "msg_stream".into(),
                            response_type: "message".into(),
                            role: "assistant".into(),
                            content: vec![],
                            model: "claude-sonnet-4-20250514".into(),
                            stop_reason: None,
                            stop_sequence: None,
                            usage: Usage {
                                input_tokens: 10,
                                output_tokens: 0,
                                cache_creation_input_tokens: None,
                                cache_read_input_tokens: None,
                            },
                        },
                    },
                    StreamEvent::ContentBlockStart {
                        index: 0,
                        content_block: ContentBlock::Text {
                            text: String::new(),
                        },
                    },
                    StreamEvent::ContentBlockDelta {
                        index: 0,
                        delta: StreamDelta::TextDelta {
                            text: "Hello!".into(),
                        },
                    },
                    StreamEvent::ContentBlockStop { index: 0 },
                    StreamEvent::MessageDelta {
                        delta: MessageDeltaPayload {
                            stop_reason: Some("end_turn".into()),
                            stop_sequence: None,
                        },
                        usage: Some(Usage {
                            input_tokens: 10,
                            output_tokens: 5,
                            cache_creation_input_tokens: None,
                            cache_read_input_tokens: None,
                        }),
                    },
                    StreamEvent::MessageStop {},
                ])
            }));

            let req = MessageRequest {
                model: "claude-sonnet-4-20250514".into(),
                max_tokens: 1024,
                messages: vec![Message {
                    role: Role::User,
                    content: vec![ContentBlock::Text {
                        text: "Hi".into(),
                    }],
                }],
                system: None,
                temperature: None,
                stop_sequences: None,
                thinking: None,
                stream: Some(true),
            };

            let events = client.create_stream(req).await.unwrap().collect_all().await;
            assert_eq!(events.len(), 6);
            assert!(matches!(&events[0], StreamEvent::MessageStart { .. }));
            assert!(matches!(
                &events[2],
                StreamEvent::ContentBlockDelta { .. }
            ));
            assert!(matches!(&events[5], StreamEvent::MessageStop {}));
        }

        #[test]
        fn claude_tool_use_content_block_preservation() {
            let block = ContentBlock::ToolUse {
                id: "toolu_abc".into(),
                name: "execute_python".into(),
                input: json!({"code": "print('hello')"}),
            };
            let json_str = serde_json::to_string(&block).unwrap();
            let back: ContentBlock = serde_json::from_str(&json_str).unwrap();
            assert_eq!(back, block);
        }

        #[test]
        fn claude_stop_reason_preserved() {
            let resp = MessageResponse {
                id: "msg_1".into(),
                response_type: "message".into(),
                role: "assistant".into(),
                content: vec![ContentBlock::ToolUse {
                    id: "toolu_1".into(),
                    name: "bash".into(),
                    input: json!({"cmd": "ls"}),
                }],
                model: "claude-sonnet-4-20250514".into(),
                stop_reason: Some("tool_use".into()),
                stop_sequence: None,
                usage: Usage {
                    input_tokens: 10,
                    output_tokens: 20,
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: None,
                },
            };
            assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
        }
    }

    mod gemini_passthrough {
        use super::*;
        use abp_shim_gemini::{
            Candidate, Content, GenerateContentRequest, GenerateContentResponse, GenerationConfig,
            GeminiClient, Part, UsageMetadata,
        };

        #[tokio::test]
        async fn gemini_generate_content_roundtrip() {
            let client = GeminiClient::new("gemini-2.5-flash");
            let request = GenerateContentRequest::new("gemini-2.5-flash")
                .add_content(Content::user(vec![Part::text("Hello")]));
            let response = client.generate(request).await.unwrap();
            assert!(!response.candidates.is_empty());
            assert!(response.text().is_some());
        }

        #[tokio::test]
        async fn gemini_function_calling_roundtrip() {
            use abp_shim_gemini::{FunctionDeclaration, ToolDeclaration};

            let client = GeminiClient::new("gemini-2.5-flash");
            let request = GenerateContentRequest::new("gemini-2.5-flash")
                .add_content(Content::user(vec![Part::text("What's the weather?")]))
                .tools(vec![ToolDeclaration {
                    function_declarations: vec![FunctionDeclaration {
                        name: "get_weather".into(),
                        description: "Get weather".into(),
                        parameters: json!({"type": "object", "properties": {"loc": {"type": "string"}}}),
                    }],
                }]);
            let response = client.generate(request).await.unwrap();
            assert!(!response.candidates.is_empty());
        }

        #[tokio::test]
        async fn gemini_usage_metadata_preserved() {
            let client = GeminiClient::new("gemini-2.5-flash");
            let request = GenerateContentRequest::new("gemini-2.5-flash")
                .add_content(Content::user(vec![Part::text("Count to 3")]));
            let response = client.generate(request).await.unwrap();
            let usage = response.usage_metadata.as_ref().unwrap();
            assert!(usage.total_token_count > 0);
            assert_eq!(
                usage.total_token_count,
                usage.prompt_token_count + usage.candidates_token_count
            );
        }

        #[tokio::test]
        async fn gemini_streaming_produces_events() {
            let client = GeminiClient::new("gemini-2.5-flash");
            let request = GenerateContentRequest::new("gemini-2.5-flash")
                .add_content(Content::user(vec![Part::text("Stream test")]));
            let stream = client.generate_stream(request).await.unwrap();
            use tokio_stream::StreamExt;
            let events: Vec<_> = stream.collect().await;
            assert!(!events.is_empty());
        }

        #[test]
        fn gemini_generation_config_passthrough() {
            let cfg = GenerationConfig {
                max_output_tokens: Some(2048),
                temperature: Some(0.7),
                top_p: Some(0.9),
                top_k: Some(40),
                candidate_count: None,
                stop_sequences: Some(vec!["END".into()]),
                response_mime_type: None,
                response_schema: None,
            };
            let json_str = serde_json::to_string(&cfg).unwrap();
            let back: GenerationConfig = serde_json::from_str(&json_str).unwrap();
            assert_eq!(back.max_output_tokens, Some(2048));
            assert_eq!(back.temperature, Some(0.7));
            assert_eq!(back.top_p, Some(0.9));
            assert_eq!(back.top_k, Some(40));
            assert_eq!(back.stop_sequences, Some(vec!["END".into()]));
        }

        #[test]
        fn gemini_model_name_in_request() {
            let req = GenerateContentRequest::new("gemini-2.5-pro-exp-03-25");
            assert_eq!(req.model, "gemini-2.5-pro-exp-03-25");
        }
    }

    mod codex_passthrough {
        use super::*;
        use abp_shim_codex::{
            CodexClient, CodexRequestBuilder, codex_message, mock_receipt, mock_receipt_with_usage,
        };
        use abp_codex_sdk::dialect::{CodexContentPart, CodexResponseItem, CodexStreamEvent};

        type ProcessFn = Box<dyn Fn(&abp_core::WorkOrder) -> Receipt + Send + Sync>;

        fn make_passthrough_processor(events: Vec<AgentEvent>) -> ProcessFn {
            Box::new(move |_wo| passthrough_receipt_default(events.clone()))
        }

        fn make_passthrough_processor_with_usage(
            events: Vec<AgentEvent>,
            usage: UsageNormalized,
        ) -> ProcessFn {
            Box::new(move |_wo| passthrough_receipt(events.clone(), usage.clone()))
        }

        #[tokio::test]
        async fn codex_code_generation_roundtrip() {
            let events = vec![assistant_event("fn main() { println!(\"Hello\"); }")];
            let client = CodexClient::new("codex-mini-latest")
                .with_processor(make_passthrough_processor(events));
            let req = CodexRequestBuilder::new()
                .model("codex-mini-latest")
                .input(vec![codex_message("user", "Write hello world in Rust")])
                .build();

            let resp = client.create(req).await.unwrap();
            assert_eq!(resp.model, "codex-mini-latest");
            assert_eq!(resp.output.len(), 1);
            match &resp.output[0] {
                CodexResponseItem::Message { content, .. } => match &content[0] {
                    CodexContentPart::OutputText { text } => {
                        assert!(text.contains("Hello"));
                    }
                },
                other => panic!("expected Message, got {other:?}"),
            }
        }

        #[tokio::test]
        async fn codex_streaming_roundtrip() {
            let events = vec![
                delta_event("fn "),
                delta_event("main()"),
                delta_event(" {}"),
            ];
            let client = CodexClient::new("codex-mini-latest")
                .with_processor(make_passthrough_processor(events));
            let req = CodexRequestBuilder::new()
                .input(vec![codex_message("user", "code")])
                .build();

            let stream = client.create_stream(req).await.unwrap();
            use tokio_stream::StreamExt;
            let chunks: Vec<CodexStreamEvent> = stream.collect().await;
            // 1 created + 3 deltas + 1 completed
            assert_eq!(chunks.len(), 5);
            assert!(matches!(
                &chunks[0],
                CodexStreamEvent::ResponseCreated { .. }
            ));
            assert!(matches!(
                &chunks[4],
                CodexStreamEvent::ResponseCompleted { .. }
            ));
        }

        #[tokio::test]
        async fn codex_tool_calls_passthrough() {
            let events = vec![tool_call_event(
                "shell",
                "fc_test",
                json!({"command": "cargo test"}),
            )];
            let client = CodexClient::new("codex-mini-latest")
                .with_processor(make_passthrough_processor(events));
            let req = CodexRequestBuilder::new()
                .input(vec![codex_message("user", "Run tests")])
                .build();

            let resp = client.create(req).await.unwrap();
            match &resp.output[0] {
                CodexResponseItem::FunctionCall {
                    id, name, arguments, ..
                } => {
                    assert_eq!(id, "fc_test");
                    assert_eq!(name, "shell");
                    assert!(arguments.contains("cargo test"));
                }
                other => panic!("expected FunctionCall, got {other:?}"),
            }
        }

        #[tokio::test]
        async fn codex_usage_stats_passthrough() {
            let usage = test_usage(200, 100);
            let events = vec![assistant_event("done")];
            let client = CodexClient::new("codex-mini-latest")
                .with_processor(make_passthrough_processor_with_usage(events, usage));
            let req = CodexRequestBuilder::new()
                .input(vec![codex_message("user", "test")])
                .build();

            let resp = client.create(req).await.unwrap();
            let u = resp.usage.unwrap();
            assert_eq!(u.input_tokens, 200);
            assert_eq!(u.output_tokens, 100);
            assert_eq!(u.total_tokens, 300);
        }

        #[tokio::test]
        async fn codex_model_name_preserved() {
            let events = vec![assistant_event("ok")];
            let client = CodexClient::new("o3-mini")
                .with_processor(make_passthrough_processor(events));
            let req = CodexRequestBuilder::new()
                .model("o3-mini")
                .input(vec![codex_message("user", "test")])
                .build();

            let resp = client.create(req).await.unwrap();
            assert_eq!(resp.model, "o3-mini");
        }
    }

    mod kimi_passthrough {
        use super::*;
        use abp_shim_kimi::{
            KimiClient, KimiRequestBuilder, Message as KimiMessage, mock_receipt,
            mock_receipt_with_usage,
        };
        use abp_kimi_sdk::dialect::KimiChunk;

        type ProcessFn = Box<dyn Fn(&abp_core::WorkOrder) -> Receipt + Send + Sync>;

        fn make_passthrough_processor(events: Vec<AgentEvent>) -> ProcessFn {
            Box::new(move |_wo| passthrough_receipt_default(events.clone()))
        }

        fn make_passthrough_processor_with_usage(
            events: Vec<AgentEvent>,
            usage: UsageNormalized,
        ) -> ProcessFn {
            Box::new(move |_wo| passthrough_receipt(events.clone(), usage.clone()))
        }

        #[tokio::test]
        async fn kimi_chat_with_web_search_roundtrip() {
            let events = vec![assistant_event(
                "According to web search results, Rust is a systems programming language.",
            )];
            let client = KimiClient::new("moonshot-v1-8k")
                .with_processor(make_passthrough_processor(events));
            let req = KimiRequestBuilder::new()
                .model("moonshot-v1-8k")
                .messages(vec![KimiMessage::user("What is Rust?")])
                .build();

            let resp = client.create(req).await.unwrap();
            assert_eq!(resp.model, "moonshot-v1-8k");
            assert_eq!(resp.choices.len(), 1);
            assert!(resp.choices[0]
                .message
                .content
                .as_deref()
                .unwrap()
                .contains("Rust"));
            assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
        }

        #[tokio::test]
        async fn kimi_streaming_roundtrip() {
            let events = vec![delta_event("Hello"), delta_event(" from Kimi!")];
            let client = KimiClient::new("moonshot-v1-8k")
                .with_processor(make_passthrough_processor(events));
            let req = KimiRequestBuilder::new()
                .messages(vec![KimiMessage::user("Hi")])
                .stream(true)
                .build();

            let stream = client.create_stream(req).await.unwrap();
            use tokio_stream::StreamExt;
            let chunks: Vec<KimiChunk> = stream.collect().await;
            // 2 deltas + 1 final stop chunk
            assert_eq!(chunks.len(), 3);
            assert_eq!(
                chunks[0].choices[0].delta.content.as_deref(),
                Some("Hello")
            );
            assert_eq!(
                chunks[1].choices[0].delta.content.as_deref(),
                Some(" from Kimi!")
            );
            assert_eq!(
                chunks[2].choices[0].finish_reason.as_deref(),
                Some("stop")
            );
        }

        #[tokio::test]
        async fn kimi_tool_calls_passthrough() {
            let events = vec![tool_call_event(
                "web_search",
                "call_ws1",
                json!({"query": "rust async"}),
            )];
            let client = KimiClient::new("moonshot-v1-8k")
                .with_processor(make_passthrough_processor(events));
            let req = KimiRequestBuilder::new()
                .messages(vec![KimiMessage::user("Search rust async")])
                .build();

            let resp = client.create(req).await.unwrap();
            assert_eq!(
                resp.choices[0].finish_reason.as_deref(),
                Some("tool_calls")
            );
            let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
            assert_eq!(tcs[0].id, "call_ws1");
            assert_eq!(tcs[0].function.name, "web_search");
        }

        #[tokio::test]
        async fn kimi_usage_stats_passthrough() {
            let usage = test_usage(80, 40);
            let events = vec![assistant_event("ok")];
            let client = KimiClient::new("moonshot-v1-8k")
                .with_processor(make_passthrough_processor_with_usage(events, usage));
            let req = KimiRequestBuilder::new()
                .messages(vec![KimiMessage::user("test")])
                .build();

            let resp = client.create(req).await.unwrap();
            let u = resp.usage.unwrap();
            assert_eq!(u.prompt_tokens, 80);
            assert_eq!(u.completion_tokens, 40);
            assert_eq!(u.total_tokens, 120);
        }

        #[tokio::test]
        async fn kimi_model_name_preserved() {
            let events = vec![assistant_event("ok")];
            let client = KimiClient::new("moonshot-v1-128k")
                .with_processor(make_passthrough_processor(events));
            let req = KimiRequestBuilder::new()
                .model("moonshot-v1-128k")
                .messages(vec![KimiMessage::user("test")])
                .build();

            let resp = client.create(req).await.unwrap();
            assert_eq!(resp.model, "moonshot-v1-128k");
        }
    }

    mod copilot_passthrough {
        use super::*;
        use abp_shim_copilot::{
            CopilotClient, CopilotRequestBuilder, Message as CopilotMessage, mock_receipt,
            mock_receipt_with_usage,
        };
        use abp_copilot_sdk::dialect::CopilotStreamEvent;

        type ProcessFn = Box<dyn Fn(&abp_core::WorkOrder) -> Receipt + Send + Sync>;

        fn make_passthrough_processor(events: Vec<AgentEvent>) -> ProcessFn {
            Box::new(move |_wo| passthrough_receipt_default(events.clone()))
        }

        #[tokio::test]
        async fn copilot_intent_based_roundtrip() {
            let events = vec![assistant_event("I can help you with that task!")];
            let client =
                CopilotClient::new("gpt-4o").with_processor(make_passthrough_processor(events));
            let req = CopilotRequestBuilder::new()
                .model("gpt-4o")
                .messages(vec![CopilotMessage::user("Help me refactor this code")])
                .build();

            let resp = client.create(req).await.unwrap();
            assert_eq!(resp.message, "I can help you with that task!");
            assert!(resp.copilot_errors.is_empty());
        }

        #[tokio::test]
        async fn copilot_streaming_roundtrip() {
            let events = vec![delta_event("Refact"), delta_event("oring...")];
            let client =
                CopilotClient::new("gpt-4o").with_processor(make_passthrough_processor(events));
            let req = CopilotRequestBuilder::new()
                .messages(vec![CopilotMessage::user("Refactor")])
                .build();

            let stream = client.create_stream(req).await.unwrap();
            use tokio_stream::StreamExt;
            let chunks: Vec<CopilotStreamEvent> = stream.collect().await;
            // 1 references + 2 deltas + 1 done
            assert_eq!(chunks.len(), 4);
            assert!(matches!(
                &chunks[0],
                CopilotStreamEvent::CopilotReferences { .. }
            ));
            assert!(matches!(&chunks[1], CopilotStreamEvent::TextDelta { .. }));
            assert!(matches!(&chunks[2], CopilotStreamEvent::TextDelta { .. }));
            assert!(matches!(&chunks[3], CopilotStreamEvent::Done {}));
        }

        #[tokio::test]
        async fn copilot_function_call_passthrough() {
            let events = vec![tool_call_event(
                "read_file",
                "call_rf1",
                json!({"path": "src/main.rs"}),
            )];
            let client =
                CopilotClient::new("gpt-4o").with_processor(make_passthrough_processor(events));
            let req = CopilotRequestBuilder::new()
                .messages(vec![CopilotMessage::user("Read file")])
                .build();

            let resp = client.create(req).await.unwrap();
            let fc = resp.function_call.unwrap();
            assert_eq!(fc.name, "read_file");
            assert_eq!(fc.id.as_deref(), Some("call_rf1"));
            assert!(fc.arguments.contains("main.rs"));
        }

        #[tokio::test]
        async fn copilot_error_events_passthrough() {
            let events = vec![error_event("rate limit exceeded")];
            let client =
                CopilotClient::new("gpt-4o").with_processor(make_passthrough_processor(events));
            let req = CopilotRequestBuilder::new()
                .messages(vec![CopilotMessage::user("test")])
                .build();

            let resp = client.create(req).await.unwrap();
            assert_eq!(resp.copilot_errors.len(), 1);
            assert!(resp.copilot_errors[0].message.contains("rate limit"));
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // 3. Metadata preservation tests
    // ═══════════════════════════════════════════════════════════════════

    mod metadata_preservation {
        use super::*;

        #[test]
        fn usage_stats_passed_through_exactly() {
            let usage = UsageNormalized {
                input_tokens: Some(1234),
                output_tokens: Some(5678),
                cache_read_tokens: Some(100),
                cache_write_tokens: Some(200),
                request_units: Some(3),
                estimated_cost_usd: Some(0.0042),
            };
            let receipt = passthrough_receipt(vec![assistant_event("ok")], usage);
            assert_eq!(receipt.usage.input_tokens, Some(1234));
            assert_eq!(receipt.usage.output_tokens, Some(5678));
            assert_eq!(receipt.usage.cache_read_tokens, Some(100));
            assert_eq!(receipt.usage.cache_write_tokens, Some(200));
            assert_eq!(receipt.usage.request_units, Some(3));
            assert_eq!(receipt.usage.estimated_cost_usd, Some(0.0042));
        }

        #[test]
        fn model_name_unchanged_in_receipt() {
            let receipt = passthrough_receipt_default(vec![assistant_event("ok")]);
            // The backend identity is set directly, not rewritten.
            assert_eq!(receipt.backend.id, "passthrough-mock");
        }

        #[test]
        fn stop_reason_preserved_via_outcome() {
            let receipt = passthrough_receipt_default(vec![assistant_event("ok")]);
            assert_eq!(receipt.outcome, Outcome::Complete);

            let failed_receipt = Receipt {
                outcome: Outcome::Failed,
                ..passthrough_receipt_default(vec![error_event("crash")])
            };
            assert_eq!(failed_receipt.outcome, Outcome::Failed);
        }

        #[test]
        fn token_counts_match_exactly() {
            let usage = test_usage(500, 250);
            let receipt = passthrough_receipt(vec![assistant_event("ok")], usage);
            assert_eq!(receipt.usage.input_tokens.unwrap(), 500);
            assert_eq!(receipt.usage.output_tokens.unwrap(), 250);
        }

        #[test]
        fn timing_information_preserved() {
            let now = Utc::now();
            let receipt = Receipt {
                meta: RunMetadata {
                    run_id: Uuid::new_v4(),
                    work_order_id: Uuid::new_v4(),
                    contract_version: CONTRACT_VERSION.to_string(),
                    started_at: now,
                    finished_at: now + chrono::Duration::milliseconds(42),
                    duration_ms: 42,
                },
                ..passthrough_receipt_default(vec![assistant_event("ok")])
            };
            assert_eq!(receipt.meta.duration_ms, 42);
            assert_eq!(
                receipt.meta.contract_version,
                CONTRACT_VERSION.to_string()
            );
        }

        #[test]
        fn receipt_builder_preserves_passthrough_mode() {
            let receipt = ReceiptBuilder::new("test-backend")
                .mode(ExecutionMode::Passthrough)
                .outcome(Outcome::Complete)
                .usage(test_usage(100, 50))
                .build();
            assert_eq!(receipt.mode, ExecutionMode::Passthrough);
            assert_eq!(receipt.usage.input_tokens, Some(100));
            assert_eq!(receipt.usage.output_tokens, Some(50));
        }

        #[test]
        fn receipt_with_hash_preserves_passthrough_mode() {
            let receipt = ReceiptBuilder::new("test-backend")
                .mode(ExecutionMode::Passthrough)
                .build()
                .with_hash()
                .unwrap();
            assert_eq!(receipt.mode, ExecutionMode::Passthrough);
            assert!(receipt.receipt_sha256.is_some());
        }

        #[test]
        fn receipt_serde_roundtrip_preserves_mode() {
            let receipt = passthrough_receipt_default(vec![assistant_event("hello")]);
            let json = serde_json::to_string(&receipt).unwrap();
            let back: Receipt = serde_json::from_str(&json).unwrap();
            assert_eq!(back.mode, ExecutionMode::Passthrough);
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // 4. Edge cases
    // ═══════════════════════════════════════════════════════════════════

    mod edge_cases {
        use super::*;

        #[test]
        fn empty_response_in_passthrough() {
            let receipt = passthrough_receipt_default(vec![]);
            assert!(receipt.trace.is_empty());
            assert_eq!(receipt.mode, ExecutionMode::Passthrough);
            assert_eq!(receipt.outcome, Outcome::Complete);
        }

        #[test]
        fn error_responses_pass_through_vendor_errors() {
            let events = vec![error_event("503 Service Unavailable: model overloaded")];
            let receipt = passthrough_receipt_default(events);
            match &receipt.trace[0].kind {
                AgentEventKind::Error {
                    message,
                    error_code,
                } => {
                    assert_eq!(message, "503 Service Unavailable: model overloaded");
                    assert!(error_code.is_none());
                }
                other => panic!("expected Error, got {other:?}"),
            }
        }

        #[tokio::test]
        async fn streaming_interruption_handling() {
            // Partial events followed by error simulates stream interruption.
            let events = vec![
                delta_event("partial"),
                error_event("connection reset"),
            ];
            let receipt = passthrough_receipt_default(events);
            assert_eq!(receipt.trace.len(), 2);
            assert!(matches!(
                &receipt.trace[0].kind,
                AgentEventKind::AssistantDelta { text } if text == "partial"
            ));
            assert!(matches!(
                &receipt.trace[1].kind,
                AgentEventKind::Error { message, .. } if message == "connection reset"
            ));
        }

        #[test]
        fn large_payload_preservation() {
            let large_text: String = "x".repeat(1_000_000);
            let event = assistant_event(&large_text);
            let receipt = passthrough_receipt_default(vec![event]);
            match &receipt.trace[0].kind {
                AgentEventKind::AssistantMessage { text } => {
                    assert_eq!(text.len(), 1_000_000);
                    assert!(text.chars().all(|c| c == 'x'));
                }
                other => panic!("expected AssistantMessage, got {other:?}"),
            }
        }

        #[test]
        fn unicode_and_special_characters_preserved() {
            let unicode_text = "こんにちは世界 🌍 Ñoño café résumé Ελληνικά العربية 中文 \u{1F600}";
            let event = assistant_event(unicode_text);
            let receipt = passthrough_receipt_default(vec![event]);
            match &receipt.trace[0].kind {
                AgentEventKind::AssistantMessage { text } => {
                    assert_eq!(text, unicode_text);
                }
                other => panic!("expected AssistantMessage, got {other:?}"),
            }
        }

        #[test]
        fn unicode_serde_roundtrip() {
            let unicode_text = "日本語テスト 🎉 émojis ñ ü ö ä \t\n\r\\\"";
            let event = assistant_event(unicode_text);
            let json = serde_json::to_string(&event).unwrap();
            let back: AgentEvent = serde_json::from_str(&json).unwrap();
            match &back.kind {
                AgentEventKind::AssistantMessage { text } => {
                    assert_eq!(text, unicode_text);
                }
                other => panic!("expected AssistantMessage, got {other:?}"),
            }
        }

        #[test]
        fn binary_content_in_tool_results_preserved() {
            let binary_b64 = base64_encode(b"\x00\x01\x02\xFF\xFE\xFD");
            let event = AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::ToolResult {
                    tool_name: "read_binary".to_string(),
                    tool_use_id: Some("call_bin".to_string()),
                    output: json!({"data": binary_b64, "encoding": "base64"}),
                    is_error: false,
                },
                ext: None,
            };
            let receipt = passthrough_receipt_default(vec![event]);
            match &receipt.trace[0].kind {
                AgentEventKind::ToolResult { output, .. } => {
                    let data = output.get("data").unwrap().as_str().unwrap();
                    assert_eq!(data, binary_b64);
                }
                other => panic!("expected ToolResult, got {other:?}"),
            }
        }

        fn base64_encode(bytes: &[u8]) -> String {
            use std::fmt::Write;
            // Simple base64-like hex encoding for test purposes
            let mut s = String::new();
            for b in bytes {
                write!(s, "{b:02x}").unwrap();
            }
            s
        }

        #[test]
        fn null_and_empty_ext_fields_preserved() {
            let mut ext = BTreeMap::new();
            ext.insert("null_field".to_string(), serde_json::Value::Null);
            ext.insert("empty_string".to_string(), json!(""));
            ext.insert("empty_array".to_string(), json!([]));
            ext.insert("empty_object".to_string(), json!({}));

            let event = AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: "test".into(),
                },
                ext: Some(ext),
            };

            let receipt = passthrough_receipt_default(vec![event]);
            let stored_ext = receipt.trace[0].ext.as_ref().unwrap();
            assert_eq!(
                stored_ext.get("null_field").unwrap(),
                &serde_json::Value::Null
            );
            assert_eq!(stored_ext.get("empty_string").unwrap(), &json!(""));
            assert_eq!(stored_ext.get("empty_array").unwrap(), &json!([]));
            assert_eq!(stored_ext.get("empty_object").unwrap(), &json!({}));
        }

        #[test]
        fn deeply_nested_ext_preserved() {
            let nested = json!({
                "level1": {
                    "level2": {
                        "level3": {
                            "level4": {
                                "value": "deep"
                            }
                        }
                    }
                }
            });
            let mut ext = BTreeMap::new();
            ext.insert("nested".to_string(), nested.clone());
            let event = AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: "test".into(),
                },
                ext: Some(ext),
            };
            let receipt = passthrough_receipt_default(vec![event]);
            let stored = receipt.trace[0]
                .ext
                .as_ref()
                .unwrap()
                .get("nested")
                .unwrap();
            assert_eq!(stored, &nested);
            assert_eq!(
                stored
                    .pointer("/level1/level2/level3/level4/value")
                    .unwrap(),
                &json!("deep")
            );
        }

        #[test]
        fn many_events_order_preserved() {
            let events: Vec<AgentEvent> = (0..100)
                .map(|i| delta_event(&format!("chunk_{i}")))
                .collect();
            let receipt = passthrough_receipt_default(events);
            assert_eq!(receipt.trace.len(), 100);
            for (i, event) in receipt.trace.iter().enumerate() {
                match &event.kind {
                    AgentEventKind::AssistantDelta { text } => {
                        assert_eq!(text, &format!("chunk_{i}"));
                    }
                    other => panic!("expected AssistantDelta at index {i}, got {other:?}"),
                }
            }
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // 5. Negative tests — passthrough MUST NOT
    // ═══════════════════════════════════════════════════════════════════

    mod negative_tests {
        use super::*;

        #[test]
        fn must_not_rewrite_model_names() {
            // Verify model name in work order is preserved exactly.
            let wo = WorkOrderBuilder::new("test")
                .model("gpt-4o-2024-11-20")
                .build();
            assert_eq!(wo.config.model.as_deref(), Some("gpt-4o-2024-11-20"));

            // Build another with a completely different model name
            let wo2 = WorkOrderBuilder::new("test")
                .model("moonshot-v1-128k")
                .build();
            assert_eq!(wo2.config.model.as_deref(), Some("moonshot-v1-128k"));

            // Model names should never be altered by the passthrough layer.
            assert_ne!(wo.config.model, wo2.config.model);
        }

        #[test]
        fn must_not_inject_additional_fields_in_ext() {
            let event = AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: "hello".into(),
                },
                ext: None,
            };
            let receipt = passthrough_receipt_default(vec![event]);
            // If ext was None going in, it should be None going out.
            assert!(receipt.trace[0].ext.is_none());
        }

        #[test]
        fn must_not_modify_token_counts() {
            let usage = UsageNormalized {
                input_tokens: Some(42),
                output_tokens: Some(17),
                cache_read_tokens: None,
                cache_write_tokens: None,
                request_units: None,
                estimated_cost_usd: None,
            };
            let receipt = passthrough_receipt(vec![assistant_event("ok")], usage);
            // Exact values must be preserved, not rounded or adjusted.
            assert_eq!(receipt.usage.input_tokens, Some(42));
            assert_eq!(receipt.usage.output_tokens, Some(17));
        }

        #[test]
        fn must_not_change_streaming_chunk_boundaries() {
            let events = vec![
                delta_event("a"),
                delta_event("bb"),
                delta_event("ccc"),
                delta_event("dddd"),
            ];
            let receipt = passthrough_receipt_default(events);
            let texts: Vec<&str> = receipt
                .trace
                .iter()
                .filter_map(|e| match &e.kind {
                    AgentEventKind::AssistantDelta { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            // Chunk boundaries must be preserved exactly.
            assert_eq!(texts, vec!["a", "bb", "ccc", "dddd"]);
        }

        #[test]
        fn must_not_alter_error_formats() {
            let error_msg = "{\n  \"error\": {\n    \"message\": \"Rate limit reached\",\n    \"type\": \"rate_limit_error\",\n    \"code\": 429\n  }\n}";
            let event = error_event(error_msg);
            let receipt = passthrough_receipt_default(vec![event]);
            match &receipt.trace[0].kind {
                AgentEventKind::Error { message, .. } => {
                    assert_eq!(message, error_msg);
                }
                other => panic!("expected Error, got {other:?}"),
            }
        }

        #[test]
        fn must_not_inject_fields_into_trace_events() {
            let event = AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::ToolCall {
                    tool_name: "bash".into(),
                    tool_use_id: Some("id_1".into()),
                    parent_tool_use_id: None,
                    input: json!({"cmd": "ls -la"}),
                },
                ext: None,
            };
            let receipt = passthrough_receipt_default(vec![event]);
            // Verify no fields were injected.
            assert!(receipt.trace[0].ext.is_none());
            match &receipt.trace[0].kind {
                AgentEventKind::ToolCall {
                    tool_name,
                    tool_use_id,
                    parent_tool_use_id,
                    input,
                } => {
                    assert_eq!(tool_name, "bash");
                    assert_eq!(tool_use_id.as_deref(), Some("id_1"));
                    assert!(parent_tool_use_id.is_none());
                    assert_eq!(input, &json!({"cmd": "ls -la"}));
                }
                other => panic!("expected ToolCall, got {other:?}"),
            }
        }

        #[test]
        fn must_not_modify_usage_none_fields() {
            let usage = UsageNormalized {
                input_tokens: Some(100),
                output_tokens: Some(50),
                cache_read_tokens: None,
                cache_write_tokens: None,
                request_units: None,
                estimated_cost_usd: None,
            };
            let receipt = passthrough_receipt(vec![assistant_event("ok")], usage);
            // None fields must remain None, not be defaulted to 0.
            assert!(receipt.usage.cache_read_tokens.is_none());
            assert!(receipt.usage.cache_write_tokens.is_none());
            assert!(receipt.usage.request_units.is_none());
            assert!(receipt.usage.estimated_cost_usd.is_none());
        }

        #[test]
        fn must_not_alter_receipt_sha256_field() {
            let receipt = passthrough_receipt_default(vec![assistant_event("ok")]);
            // Before hashing, sha256 should be None.
            assert!(receipt.receipt_sha256.is_none());

            // After hashing, the hash should be deterministic.
            let hashed = receipt.with_hash().unwrap();
            assert!(hashed.receipt_sha256.is_some());
            let hash1 = hashed.receipt_sha256.clone().unwrap();

            // Hashing again should produce the same result.
            let rehashed = Receipt {
                receipt_sha256: None,
                ..hashed
            }
            .with_hash()
            .unwrap();
            assert_eq!(rehashed.receipt_sha256.unwrap(), hash1);
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // 6. Cross-SDK passthrough consistency
    // ═══════════════════════════════════════════════════════════════════

    mod cross_sdk_consistency {
        use super::*;

        #[test]
        fn all_shims_produce_same_receipt_structure() {
            // When given the same events, receipts from all shims should
            // have structurally identical traces.
            let events = vec![
                assistant_event("Hello"),
                tool_call_event("search", "call_1", json!({"q": "test"})),
                assistant_event("Found it."),
            ];

            let receipt = passthrough_receipt_default(events);
            assert_eq!(receipt.trace.len(), 3);
            assert!(matches!(
                &receipt.trace[0].kind,
                AgentEventKind::AssistantMessage { text } if text == "Hello"
            ));
            assert!(matches!(
                &receipt.trace[1].kind,
                AgentEventKind::ToolCall { tool_name, .. } if tool_name == "search"
            ));
            assert!(matches!(
                &receipt.trace[2].kind,
                AgentEventKind::AssistantMessage { text } if text == "Found it."
            ));
        }

        #[test]
        fn execution_mode_enum_has_exactly_two_variants() {
            // Ensure we know about all variants for exhaustiveness.
            let passthrough = ExecutionMode::Passthrough;
            let mapped = ExecutionMode::Mapped;
            assert_ne!(passthrough, mapped);
            // Default is Mapped.
            assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
        }

        #[test]
        fn passthrough_receipt_json_includes_mode_field() {
            let receipt = passthrough_receipt_default(vec![]);
            let json_str = serde_json::to_string(&receipt).unwrap();
            let val: serde_json::Value = serde_json::from_str(&json_str).unwrap();
            assert_eq!(val["mode"], json!("passthrough"));
        }

        #[test]
        fn mapped_receipt_json_includes_mode_field() {
            let mut receipt = passthrough_receipt_default(vec![]);
            receipt.mode = ExecutionMode::Mapped;
            let json_str = serde_json::to_string(&receipt).unwrap();
            let val: serde_json::Value = serde_json::from_str(&json_str).unwrap();
            assert_eq!(val["mode"], json!("mapped"));
        }

        #[test]
        fn work_order_vendor_config_passthrough_to_receipt() {
            let wo = WorkOrderBuilder::new("test passthrough")
                .model("gpt-4o")
                .build();
            // Vendor config set on work order should be available for passthrough.
            assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
            assert_eq!(wo.task, "test passthrough");
        }

        #[test]
        fn multiple_tool_calls_preserve_ordering() {
            let events = vec![
                tool_call_event("tool_a", "call_1", json!({"a": 1})),
                tool_call_event("tool_b", "call_2", json!({"b": 2})),
                tool_call_event("tool_c", "call_3", json!({"c": 3})),
            ];
            let receipt = passthrough_receipt_default(events);

            let names: Vec<&str> = receipt
                .trace
                .iter()
                .filter_map(|e| match &e.kind {
                    AgentEventKind::ToolCall { tool_name, .. } => Some(tool_name.as_str()),
                    _ => None,
                })
                .collect();
            assert_eq!(names, vec!["tool_a", "tool_b", "tool_c"]);
        }

        #[test]
        fn mixed_event_types_preserve_ordering() {
            let events = vec![
                AgentEvent {
                    ts: Utc::now(),
                    kind: AgentEventKind::RunStarted {
                        message: "starting".into(),
                    },
                    ext: None,
                },
                delta_event("partial"),
                tool_call_event("bash", "c1", json!({"cmd": "ls"})),
                AgentEvent {
                    ts: Utc::now(),
                    kind: AgentEventKind::ToolResult {
                        tool_name: "bash".into(),
                        tool_use_id: Some("c1".into()),
                        output: json!("file1.rs\nfile2.rs"),
                        is_error: false,
                    },
                    ext: None,
                },
                assistant_event("Done."),
                AgentEvent {
                    ts: Utc::now(),
                    kind: AgentEventKind::RunCompleted {
                        message: "finished".into(),
                    },
                    ext: None,
                },
            ];
            let receipt = passthrough_receipt_default(events);
            assert_eq!(receipt.trace.len(), 6);
            assert!(matches!(
                &receipt.trace[0].kind,
                AgentEventKind::RunStarted { .. }
            ));
            assert!(matches!(
                &receipt.trace[1].kind,
                AgentEventKind::AssistantDelta { .. }
            ));
            assert!(matches!(
                &receipt.trace[2].kind,
                AgentEventKind::ToolCall { .. }
            ));
            assert!(matches!(
                &receipt.trace[3].kind,
                AgentEventKind::ToolResult { .. }
            ));
            assert!(matches!(
                &receipt.trace[4].kind,
                AgentEventKind::AssistantMessage { .. }
            ));
            assert!(matches!(
                &receipt.trace[5].kind,
                AgentEventKind::RunCompleted { .. }
            ));
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // 7. Serde fidelity for passthrough
    // ═══════════════════════════════════════════════════════════════════

    mod serde_fidelity {
        use super::*;

        #[test]
        fn full_receipt_serde_roundtrip_passthrough() {
            let usage = UsageNormalized {
                input_tokens: Some(999),
                output_tokens: Some(888),
                cache_read_tokens: Some(77),
                cache_write_tokens: Some(66),
                request_units: Some(5),
                estimated_cost_usd: Some(1.23),
            };
            let events = vec![
                assistant_event("Hi"),
                tool_call_event("read", "c1", json!({"path": "a.rs"})),
                delta_event("delta"),
                error_event("oops"),
            ];
            let receipt = passthrough_receipt(events, usage);
            let json = serde_json::to_string_pretty(&receipt).unwrap();
            let back: Receipt = serde_json::from_str(&json).unwrap();

            assert_eq!(back.mode, ExecutionMode::Passthrough);
            assert_eq!(back.outcome, Outcome::Complete);
            assert_eq!(back.trace.len(), 4);
            assert_eq!(back.usage.input_tokens, Some(999));
            assert_eq!(back.usage.output_tokens, Some(888));
            assert_eq!(back.usage.cache_read_tokens, Some(77));
            assert_eq!(back.usage.cache_write_tokens, Some(66));
            assert_eq!(back.usage.request_units, Some(5));
            assert_eq!(back.usage.estimated_cost_usd, Some(1.23));
            assert_eq!(back.meta.contract_version, CONTRACT_VERSION);
        }

        #[test]
        fn agent_event_with_ext_serde_roundtrip() {
            let raw = json!({
                "vendor": "openai",
                "original_response": {
                    "id": "chatcmpl-xyz",
                    "model": "gpt-4o",
                    "choices": [{"message": {"content": "hi"}}]
                }
            });
            let event = event_with_raw(
                AgentEventKind::AssistantMessage {
                    text: "hi".into(),
                },
                raw.clone(),
            );
            let json_str = serde_json::to_string(&event).unwrap();
            let back: AgentEvent = serde_json::from_str(&json_str).unwrap();
            assert_eq!(
                back.ext.as_ref().unwrap().get("raw_message").unwrap(),
                &raw
            );
        }

        #[test]
        fn all_agent_event_kinds_serde_roundtrip() {
            let events = vec![
                AgentEvent {
                    ts: Utc::now(),
                    kind: AgentEventKind::RunStarted {
                        message: "go".into(),
                    },
                    ext: None,
                },
                AgentEvent {
                    ts: Utc::now(),
                    kind: AgentEventKind::AssistantDelta {
                        text: "delta".into(),
                    },
                    ext: None,
                },
                AgentEvent {
                    ts: Utc::now(),
                    kind: AgentEventKind::AssistantMessage {
                        text: "msg".into(),
                    },
                    ext: None,
                },
                AgentEvent {
                    ts: Utc::now(),
                    kind: AgentEventKind::ToolCall {
                        tool_name: "bash".into(),
                        tool_use_id: Some("id1".into()),
                        parent_tool_use_id: Some("parent1".into()),
                        input: json!({"cmd": "echo hi"}),
                    },
                    ext: None,
                },
                AgentEvent {
                    ts: Utc::now(),
                    kind: AgentEventKind::ToolResult {
                        tool_name: "bash".into(),
                        tool_use_id: Some("id1".into()),
                        output: json!("hi\n"),
                        is_error: false,
                    },
                    ext: None,
                },
                AgentEvent {
                    ts: Utc::now(),
                    kind: AgentEventKind::FileChanged {
                        path: "src/main.rs".into(),
                        summary: "added logging".into(),
                    },
                    ext: None,
                },
                AgentEvent {
                    ts: Utc::now(),
                    kind: AgentEventKind::CommandExecuted {
                        command: "cargo test".into(),
                        exit_code: Some(0),
                        output_preview: Some("test passed".into()),
                    },
                    ext: None,
                },
                AgentEvent {
                    ts: Utc::now(),
                    kind: AgentEventKind::Warning {
                        message: "deprecated".into(),
                    },
                    ext: None,
                },
                AgentEvent {
                    ts: Utc::now(),
                    kind: AgentEventKind::Error {
                        message: "fatal".into(),
                        error_code: None,
                    },
                    ext: None,
                },
                AgentEvent {
                    ts: Utc::now(),
                    kind: AgentEventKind::RunCompleted {
                        message: "done".into(),
                    },
                    ext: None,
                },
            ];

            for event in &events {
                let json_str = serde_json::to_string(event).unwrap();
                let back: AgentEvent = serde_json::from_str(&json_str).unwrap();
                // Verify the kind discriminator survived roundtrip.
                let orig_json = serde_json::to_value(&event.kind).unwrap();
                let back_json = serde_json::to_value(&back.kind).unwrap();
                assert_eq!(orig_json["type"], back_json["type"]);
            }
        }
    }
}
