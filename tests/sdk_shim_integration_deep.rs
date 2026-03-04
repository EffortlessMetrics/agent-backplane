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
//! Deep integration tests for all 6 SDK shim crates.
//!
//! Covers serde roundtrips, From conversions (request→WorkOrder, Receipt→response),
//! streaming types, edge cases, and cross-crate invariants for:
//! OpenAI, Claude, Gemini, Kimi, Codex, and Copilot.

#![allow(
    clippy::useless_vec,
    clippy::needless_borrows_for_generic_args,
    clippy::collapsible_if
)]

use std::collections::BTreeMap;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, ExecutionMode, Outcome,
    Receipt, RunMetadata, UsageNormalized, VerificationReport, WorkOrder,
};
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn make_receipt(
    backend_id: &str,
    trace: Vec<AgentEvent>,
    usage: UsageNormalized,
    outcome: Outcome,
) -> Receipt {
    let now = Utc::now();
    Receipt {
        meta: RunMetadata {
            run_id: Uuid::new_v4(),
            work_order_id: Uuid::new_v4(),
            contract_version: "abp/v0.1".into(),
            started_at: now,
            finished_at: now,
            duration_ms: 42,
        },
        backend: BackendIdentity {
            id: backend_id.into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::Mapped,
        usage_raw: json!({}),
        usage,
        trace,
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome,
        receipt_sha256: None,
    }
}

fn assistant_event(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: text.into() },
        ext: None,
    }
}

fn tool_call_event(name: &str, id: Option<&str>, input: serde_json::Value) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: name.into(),
            tool_use_id: id.map(String::from),
            parent_tool_use_id: None,
            input,
        },
        ext: None,
    }
}

fn tool_result_event(name: &str, id: Option<&str>, output: serde_json::Value) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolResult {
            tool_name: name.into(),
            tool_use_id: id.map(String::from),
            output,
            is_error: false,
        },
        ext: None,
    }
}

fn error_event(msg: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: msg.into(),
            error_code: None,
        },
        ext: None,
    }
}

fn usage(input: u64, output: u64) -> UsageNormalized {
    UsageNormalized {
        input_tokens: Some(input),
        output_tokens: Some(output),
        ..UsageNormalized::default()
    }
}

// ===========================================================================
// 1. OpenAI SDK (abp_openai_sdk::api) — 20 tests
// ===========================================================================

mod openai {
    use super::*;
    use abp_openai_sdk::api::*;
    use abp_openai_sdk::response_format::{JsonSchemaSpec, ResponseFormat};

    // -- Serde roundtrips --

    #[test]
    fn system_message_serde_roundtrip() {
        let msg = Message::System {
            content: "You are helpful.".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""role":"system""#));
        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, msg);
    }

    #[test]
    fn user_message_serde_roundtrip() {
        let msg = Message::User {
            content: "Hello there".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""role":"user""#));
        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, msg);
    }

    #[test]
    fn assistant_message_with_content_serde_roundtrip() {
        let msg = Message::Assistant {
            content: Some("Sure!".into()),
            tool_calls: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, msg);
    }

    #[test]
    fn assistant_message_with_tool_calls_serde_roundtrip() {
        let msg = Message::Assistant {
            content: None,
            tool_calls: Some(vec![ToolCall {
                id: "call_1".into(),
                call_type: "function".into(),
                function: FunctionCall {
                    name: "read_file".into(),
                    arguments: r#"{"path":"main.rs"}"#.into(),
                },
            }]),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, msg);
    }

    #[test]
    fn tool_message_serde_roundtrip() {
        let msg = Message::Tool {
            tool_call_id: "call_1".into(),
            content: "file contents here".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""role":"tool""#));
        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, msg);
    }

    #[test]
    fn chat_completion_request_full_serde_roundtrip() {
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![
                Message::System {
                    content: "Be concise.".into(),
                },
                Message::User {
                    content: "Summarize Rust".into(),
                },
            ],
            temperature: Some(0.7),
            max_tokens: Some(2048),
            tools: Some(vec![Tool {
                tool_type: "function".into(),
                function: FunctionDefinition {
                    name: "search".into(),
                    description: Some("Web search".into()),
                    parameters: Some(json!({"type": "object"})),
                    strict: Some(true),
                },
            }]),
            tool_choice: Some(json!("auto")),
            stream: Some(false),
            top_p: Some(0.9),
            frequency_penalty: Some(0.1),
            presence_penalty: Some(0.2),
            stop: Some(vec!["###".into()]),
            n: Some(1),
            seed: Some(42),
            response_format: Some(ResponseFormat::JsonObject),
            user: Some("user_123".into()),
            stream_options: None,
            parallel_tool_calls: None,
            service_tier: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: ChatCompletionRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, req);
    }

    #[test]
    fn chat_completion_request_minimal_serde_roundtrip() {
        let req = ChatCompletionRequest {
            model: "gpt-4o-mini".into(),
            messages: vec![Message::User {
                content: "Hi".into(),
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
            stream_options: None,
            parallel_tool_calls: None,
            service_tier: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.contains("temperature"));
        assert!(!json.contains("max_tokens"));
        let parsed: ChatCompletionRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, req);
    }

    #[test]
    fn chat_completion_response_serde_roundtrip() {
        let resp = ChatCompletionResponse {
            id: "chatcmpl-123".into(),
            object: "chat.completion".into(),
            created: 1700000000,
            model: "gpt-4o".into(),
            choices: vec![Choice {
                index: 0,
                message: AssistantMessage {
                    role: "assistant".into(),
                    content: Some("Hello!".into()),
                    tool_calls: None,
                },
                finish_reason: FinishReason::Stop,
                safety_ratings: None,
            }],
            usage: Some(Usage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
                prompt_tokens_details: None,
                completion_tokens_details: None,
            }),
            system_fingerprint: Some("fp_abc".into()),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: ChatCompletionResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, resp);
    }

    #[test]
    fn finish_reason_variants_serde() {
        for (variant, expected) in [
            (FinishReason::Stop, r#""stop""#),
            (FinishReason::Length, r#""length""#),
            (FinishReason::ToolCalls, r#""tool_calls""#),
            (FinishReason::ContentFilter, r#""content_filter""#),
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, expected);
            let parsed: FinishReason = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, variant);
        }
    }

    #[test]
    fn response_format_json_schema_serde() {
        let fmt = ResponseFormat::JsonSchema {
            json_schema: JsonSchemaSpec {
                name: "output".into(),
                description: Some("Structured output".into()),
                schema: json!({"type": "object", "properties": {"answer": {"type": "string"}}}),
                strict: Some(true),
            },
        };
        let json = serde_json::to_string(&fmt).unwrap();
        let parsed: ResponseFormat = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, fmt);
    }

    // -- StreamChunk serde --

    #[test]
    fn stream_chunk_serde_roundtrip() {
        let chunk = StreamChunk {
            id: "chatcmpl-stream-1".into(),
            object: "chat.completion.chunk".into(),
            created: 1700000000,
            model: "gpt-4o".into(),
            choices: vec![StreamChoice {
                index: 0,
                delta: Delta {
                    role: Some("assistant".into()),
                    content: Some("Hello".into()),
                    tool_calls: None,
                },
                finish_reason: None,
                safety_ratings: None,
            }],
            usage: None,
        };
        let json = serde_json::to_string(&chunk).unwrap();
        let parsed: StreamChunk = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, chunk);
    }

    #[test]
    fn stream_chunk_with_finish_reason_and_usage() {
        let chunk = StreamChunk {
            id: "chatcmpl-stream-2".into(),
            object: "chat.completion.chunk".into(),
            created: 1700000000,
            model: "gpt-4o".into(),
            choices: vec![StreamChoice {
                index: 0,
                delta: Delta::default(),
                finish_reason: Some(FinishReason::Stop),
                safety_ratings: None,
            }],
            usage: Some(Usage {
                prompt_tokens: 50,
                completion_tokens: 25,
                total_tokens: 75,
                prompt_tokens_details: None,
                completion_tokens_details: None,
            }),
        };
        let json = serde_json::to_string(&chunk).unwrap();
        let parsed: StreamChunk = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, chunk);
    }

    #[test]
    fn stream_chunk_delta_with_tool_calls() {
        let chunk = StreamChunk {
            id: "chatcmpl-stream-3".into(),
            object: "chat.completion.chunk".into(),
            created: 1700000000,
            model: "gpt-4o".into(),
            choices: vec![StreamChoice {
                index: 0,
                delta: Delta {
                    role: None,
                    content: None,
                    tool_calls: Some(vec![ToolCall {
                        id: "call_1".into(),
                        call_type: "function".into(),
                        function: FunctionCall {
                            name: "bash".into(),
                            arguments: r#"{"cmd":"ls"}"#.into(),
                        },
                    }]),
                },
                finish_reason: None,
                safety_ratings: None,
            }],
            usage: None,
        };
        let json = serde_json::to_string(&chunk).unwrap();
        let parsed: StreamChunk = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, chunk);
    }

    // -- From<ChatCompletionRequest> for WorkOrder --

    #[test]
    fn request_to_work_order_extracts_last_user_message() {
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![
                Message::User {
                    content: "First".into(),
                },
                Message::User {
                    content: "Second".into(),
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
            stream_options: None,
            parallel_tool_calls: None,
            service_tier: None,
        };
        let wo: WorkOrder = req.into();
        assert_eq!(wo.task, "Second");
    }

    #[test]
    fn request_to_work_order_maps_system_to_snippets() {
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![
                Message::System {
                    content: "You are a bot.".into(),
                },
                Message::User {
                    content: "Hi".into(),
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
            stream_options: None,
            parallel_tool_calls: None,
            service_tier: None,
        };
        let wo: WorkOrder = req.into();
        assert_eq!(wo.context.snippets.len(), 1);
        assert_eq!(wo.context.snippets[0].content, "You are a bot.");
    }

    #[test]
    fn request_to_work_order_preserves_model() {
        let req = ChatCompletionRequest {
            model: "gpt-4-turbo".into(),
            messages: vec![Message::User {
                content: "Test".into(),
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
            stream_options: None,
            parallel_tool_calls: None,
            service_tier: None,
        };
        let wo: WorkOrder = req.into();
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4-turbo"));
    }

    #[test]
    fn request_to_work_order_empty_messages() {
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![],
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
            stream_options: None,
            parallel_tool_calls: None,
            service_tier: None,
        };
        let wo: WorkOrder = req.into();
        assert_eq!(wo.task, "");
    }

    // -- From<Receipt> for ChatCompletionResponse --

    #[test]
    fn receipt_to_response_maps_assistant_text() {
        let receipt = make_receipt(
            "openai/gpt-4o",
            vec![assistant_event("Hello!")],
            UsageNormalized::default(),
            Outcome::Complete,
        );
        let resp: ChatCompletionResponse = receipt.into();
        assert_eq!(resp.object, "chat.completion");
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hello!"));
        assert_eq!(resp.choices[0].finish_reason, FinishReason::Stop);
    }

    #[test]
    fn receipt_to_response_maps_tool_calls() {
        let receipt = make_receipt(
            "openai/gpt-4o",
            vec![tool_call_event(
                "bash",
                Some("call_abc"),
                json!({"cmd": "ls"}),
            )],
            UsageNormalized::default(),
            Outcome::Complete,
        );
        let resp: ChatCompletionResponse = receipt.into();
        assert_eq!(resp.choices[0].finish_reason, FinishReason::ToolCalls);
        let tc = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tc.len(), 1);
        assert_eq!(tc[0].id, "call_abc");
        assert_eq!(tc[0].function.name, "bash");
    }

    #[test]
    fn receipt_to_response_maps_usage() {
        let receipt = make_receipt("openai/gpt-4o", vec![], usage(100, 50), Outcome::Complete);
        let resp: ChatCompletionResponse = receipt.into();
        let u = resp.usage.unwrap();
        assert_eq!(u.prompt_tokens, 100);
        assert_eq!(u.completion_tokens, 50);
        assert_eq!(u.total_tokens, 150);
    }

    #[test]
    fn receipt_to_response_id_format() {
        let receipt = make_receipt(
            "openai/gpt-4o",
            vec![],
            UsageNormalized::default(),
            Outcome::Complete,
        );
        let run_id = receipt.meta.run_id;
        let resp: ChatCompletionResponse = receipt.into();
        assert!(resp.id.starts_with("chatcmpl-"));
        assert!(resp.id.contains(&run_id.to_string()));
    }
}

// ===========================================================================
// 2. Claude SDK (abp_claude_sdk::messages) — 20 tests
// ===========================================================================

mod claude {
    use super::*;
    use abp_claude_sdk::messages::*;

    // -- Serde roundtrips --

    #[test]
    fn messages_request_minimal_serde_roundtrip() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![Message {
                role: Role::User,
                content: MessageContent::Text("Hello".into()),
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
            tool_choice: None,
            thinking: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: MessagesRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, req);
    }

    #[test]
    fn messages_request_full_serde_roundtrip() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![
                Message {
                    role: Role::User,
                    content: MessageContent::Text("Analyze this code".into()),
                },
                Message {
                    role: Role::Assistant,
                    content: MessageContent::Blocks(vec![ContentBlock::ToolUse {
                        id: "tu_1".into(),
                        name: "read_file".into(),
                        input: json!({"path": "lib.rs"}),
                    }]),
                },
                Message {
                    role: Role::User,
                    content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                        tool_use_id: "tu_1".into(),
                        content: Some("fn main() {}".into()),
                        is_error: None,
                    }]),
                },
            ],
            max_tokens: 4096,
            system: Some(SystemMessage::Text("Be helpful.".into())),
            tools: Some(vec![Tool {
                name: "read_file".into(),
                description: "Read a file".into(),
                input_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
            }]),
            metadata: Some(Metadata {
                user_id: Some("user_42".into()),
            }),
            stream: Some(true),
            stop_sequences: Some(vec!["---".into()]),
            temperature: Some(0.5),
            top_p: Some(0.9),
            top_k: Some(40),
            tool_choice: None,
            thinking: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: MessagesRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, req);
    }

    #[test]
    fn system_message_text_serde_roundtrip() {
        let sys = SystemMessage::Text("You are a coding assistant.".into());
        let json = serde_json::to_string(&sys).unwrap();
        let parsed: SystemMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, sys);
    }

    #[test]
    fn system_message_blocks_with_cache_control_serde() {
        let sys = SystemMessage::Blocks(vec![SystemBlock::Text {
            text: "System prompt".into(),
            cache_control: Some(CacheControl {
                cache_type: "ephemeral".into(),
            }),
        }]);
        let json = serde_json::to_string(&sys).unwrap();
        assert!(json.contains("ephemeral"));
        let parsed: SystemMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, sys);
    }

    #[test]
    fn content_block_text_serde() {
        let block = ContentBlock::Text {
            text: "Hello world".into(),
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains(r#""type":"text""#));
        let parsed: ContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, block);
    }

    #[test]
    fn content_block_tool_use_serde() {
        let block = ContentBlock::ToolUse {
            id: "tu_abc".into(),
            name: "bash".into(),
            input: json!({"command": "ls -la"}),
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains(r#""type":"tool_use""#));
        let parsed: ContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, block);
    }

    #[test]
    fn content_block_tool_result_serde() {
        let block = ContentBlock::ToolResult {
            tool_use_id: "tu_abc".into(),
            content: Some("file content".into()),
            is_error: Some(false),
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains(r#""type":"tool_result""#));
        let parsed: ContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, block);
    }

    #[test]
    fn content_block_thinking_serde() {
        let block = ContentBlock::Thinking {
            thinking: "Let me reason about this...".into(),
            signature: Some("sig_12345".into()),
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains(r#""type":"thinking""#));
        let parsed: ContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, block);
    }

    #[test]
    fn content_block_image_base64_serde() {
        let block = ContentBlock::Image {
            source: ImageSource::Base64 {
                media_type: "image/png".into(),
                data: "iVBORw0KGgo=".into(),
            },
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains(r#""type":"image""#));
        let parsed: ContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, block);
    }

    #[test]
    fn content_block_image_url_serde() {
        let block = ContentBlock::Image {
            source: ImageSource::Url {
                url: "https://example.com/img.png".into(),
            },
        };
        let json = serde_json::to_string(&block).unwrap();
        let parsed: ContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, block);
    }

    #[test]
    fn message_content_text_serde() {
        let mc = MessageContent::Text("plain text".into());
        let json = serde_json::to_string(&mc).unwrap();
        let parsed: MessageContent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, mc);
    }

    #[test]
    fn message_content_blocks_serde() {
        let mc = MessageContent::Blocks(vec![
            ContentBlock::Text {
                text: "Look:".into(),
            },
            ContentBlock::Image {
                source: ImageSource::Base64 {
                    media_type: "image/jpeg".into(),
                    data: "abc123".into(),
                },
            },
        ]);
        let json = serde_json::to_string(&mc).unwrap();
        let parsed: MessageContent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, mc);
    }

    #[test]
    fn messages_response_serde_roundtrip() {
        let resp = MessagesResponse {
            id: "msg_test".into(),
            response_type: "message".into(),
            role: "assistant".into(),
            content: vec![ContentBlock::Text {
                text: "Done!".into(),
            }],
            model: "claude-sonnet-4-20250514".into(),
            stop_reason: Some("end_turn".into()),
            stop_sequence: None,
            usage: Usage {
                input_tokens: 100,
                output_tokens: 50,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: MessagesResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, resp);
    }

    #[test]
    fn role_variants_serde() {
        let u = Role::User;
        let a = Role::Assistant;
        assert_eq!(serde_json::to_string(&u).unwrap(), r#""user""#);
        assert_eq!(serde_json::to_string(&a).unwrap(), r#""assistant""#);
    }

    // -- From<MessagesRequest> for WorkOrder --

    #[test]
    fn request_to_work_order_extracts_user_text() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![Message {
                role: Role::User,
                content: MessageContent::Text("Fix the bug".into()),
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
            tool_choice: None,
            thinking: None,
        };
        let wo: WorkOrder = req.into();
        assert!(wo.task.contains("Fix the bug"));
    }

    #[test]
    fn request_to_work_order_stores_tools_in_vendor() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![Message {
                role: Role::User,
                content: MessageContent::Text("Help".into()),
            }],
            max_tokens: 1024,
            system: None,
            tools: Some(vec![Tool {
                name: "bash".into(),
                description: "Run shell".into(),
                input_schema: json!({"type": "object"}),
            }]),
            metadata: None,
            stream: None,
            stop_sequences: None,
            temperature: None,
            top_p: None,
            top_k: None,
            tool_choice: None,
            thinking: None,
        };
        let wo: WorkOrder = req.into();
        assert!(wo.config.vendor.contains_key("tools"));
    }

    #[test]
    fn request_to_work_order_stores_system_in_vendor() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![Message {
                role: Role::User,
                content: MessageContent::Text("Go".into()),
            }],
            max_tokens: 1024,
            system: Some(SystemMessage::Text("Be brief.".into())),
            tools: None,
            metadata: None,
            stream: None,
            stop_sequences: None,
            temperature: None,
            top_p: None,
            top_k: None,
            tool_choice: None,
            thinking: None,
        };
        let wo: WorkOrder = req.into();
        assert!(wo.config.vendor.contains_key("system"));
    }

    #[test]
    fn request_to_work_order_stores_metadata_in_vendor() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![Message {
                role: Role::User,
                content: MessageContent::Text("Test".into()),
            }],
            max_tokens: 1024,
            system: None,
            tools: None,
            metadata: Some(Metadata {
                user_id: Some("usr_1".into()),
            }),
            stream: None,
            stop_sequences: None,
            temperature: None,
            top_p: None,
            top_k: None,
            tool_choice: None,
            thinking: None,
        };
        let wo: WorkOrder = req.into();
        assert!(wo.config.vendor.contains_key("metadata"));
    }

    // -- From<Receipt> for MessagesResponse --

    #[test]
    fn receipt_to_response_maps_text_blocks() {
        let receipt = make_receipt(
            "anthropic/claude-sonnet-4",
            vec![assistant_event("Hello from Claude!")],
            usage(200, 100),
            Outcome::Complete,
        );
        let resp: MessagesResponse = receipt.into();
        assert_eq!(resp.response_type, "message");
        assert_eq!(resp.role, "assistant");
        assert_eq!(resp.content.len(), 1);
        match &resp.content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "Hello from Claude!"),
            other => panic!("expected Text, got {other:?}"),
        }
        assert_eq!(resp.stop_reason.as_deref(), Some("end_turn"));
    }

    #[test]
    fn receipt_to_response_maps_tool_use() {
        let receipt = make_receipt(
            "anthropic/claude-sonnet-4",
            vec![tool_call_event("bash", Some("tu_1"), json!({"cmd": "ls"}))],
            usage(100, 50),
            Outcome::Complete,
        );
        let resp: MessagesResponse = receipt.into();
        assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
        match &resp.content[0] {
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "tu_1");
                assert_eq!(name, "bash");
                assert_eq!(input, &json!({"cmd": "ls"}));
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn receipt_to_response_maps_usage_with_cache() {
        let u = UsageNormalized {
            input_tokens: Some(500),
            output_tokens: Some(200),
            cache_read_tokens: Some(100),
            cache_write_tokens: Some(50),
            ..UsageNormalized::default()
        };
        let receipt = make_receipt("anthropic/claude-sonnet-4", vec![], u, Outcome::Complete);
        let resp: MessagesResponse = receipt.into();
        assert_eq!(resp.usage.input_tokens, 500);
        assert_eq!(resp.usage.output_tokens, 200);
        assert_eq!(resp.usage.cache_read_input_tokens, Some(100));
        assert_eq!(resp.usage.cache_creation_input_tokens, Some(50));
    }
}

// ===========================================================================
// 3. Gemini SDK (abp_gemini_sdk) — 20 tests
// ===========================================================================

mod gemini {
    use super::*;
    use abp_gemini_sdk::dialect::*;

    // -- Serde roundtrips --

    #[test]
    fn gemini_request_minimal_serde_roundtrip() {
        let req = GeminiRequest {
            model: "gemini-2.5-flash".into(),
            contents: vec![GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::Text("Hello".into())],
            }],
            system_instruction: None,
            generation_config: None,
            safety_settings: None,
            tools: None,
            tool_config: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: GeminiRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.model, req.model);
        assert_eq!(parsed.contents.len(), 1);
    }

    #[test]
    fn gemini_request_full_serde_roundtrip() {
        let req = GeminiRequest {
            model: "gemini-2.5-pro".into(),
            contents: vec![GeminiContent {
                role: "user".into(),
                parts: vec![
                    GeminiPart::Text("Look at this image".into()),
                    GeminiPart::InlineData(GeminiInlineData {
                        mime_type: "image/png".into(),
                        data: "base64data".into(),
                    }),
                ],
            }],
            system_instruction: Some(GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::Text("Be concise.".into())],
            }),
            generation_config: Some(GeminiGenerationConfig {
                max_output_tokens: Some(2048),
                temperature: Some(0.7),
                top_p: Some(0.9),
                top_k: Some(40),
                candidate_count: Some(1),
                stop_sequences: Some(vec!["END".into()]),
                response_mime_type: Some("application/json".into()),
                response_schema: Some(json!({"type": "object"})),
            }),
            safety_settings: Some(vec![GeminiSafetySetting {
                category: HarmCategory::HarmCategoryHarassment,
                threshold: HarmBlockThreshold::BlockMediumAndAbove,
            }]),
            tools: Some(vec![GeminiTool {
                function_declarations: vec![GeminiFunctionDeclaration {
                    name: "search".into(),
                    description: "Search the web".into(),
                    parameters: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
                }],
            }]),
            tool_config: Some(GeminiToolConfig {
                function_calling_config: GeminiFunctionCallingConfig {
                    mode: FunctionCallingMode::Auto,
                    allowed_function_names: None,
                },
            }),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: GeminiRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.model, "gemini-2.5-pro");
        assert!(parsed.generation_config.is_some());
        assert!(parsed.safety_settings.is_some());
    }

    #[test]
    fn gemini_part_text_serde() {
        let part = GeminiPart::Text("hello".into());
        let json = serde_json::to_string(&part).unwrap();
        let parsed: GeminiPart = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, GeminiPart::Text(t) if t == "hello"));
    }

    #[test]
    fn gemini_part_inline_data_serde() {
        let part = GeminiPart::InlineData(GeminiInlineData {
            mime_type: "image/jpeg".into(),
            data: "abcdef".into(),
        });
        let json = serde_json::to_string(&part).unwrap();
        let parsed: GeminiPart = serde_json::from_str(&json).unwrap();
        match parsed {
            GeminiPart::InlineData(d) => {
                assert_eq!(d.mime_type, "image/jpeg");
                assert_eq!(d.data, "abcdef");
            }
            other => panic!("expected InlineData, got {other:?}"),
        }
    }

    #[test]
    fn gemini_part_function_call_serde() {
        let part = GeminiPart::FunctionCall {
            name: "get_weather".into(),
            args: json!({"location": "NYC"}),
        };
        let json = serde_json::to_string(&part).unwrap();
        let parsed: GeminiPart = serde_json::from_str(&json).unwrap();
        match parsed {
            GeminiPart::FunctionCall { name, args } => {
                assert_eq!(name, "get_weather");
                assert_eq!(args["location"], "NYC");
            }
            other => panic!("expected FunctionCall, got {other:?}"),
        }
    }

    #[test]
    fn gemini_part_function_response_serde() {
        let part = GeminiPart::FunctionResponse {
            name: "get_weather".into(),
            response: json!({"temp": 72}),
        };
        let json = serde_json::to_string(&part).unwrap();
        let parsed: GeminiPart = serde_json::from_str(&json).unwrap();
        match parsed {
            GeminiPart::FunctionResponse { name, response } => {
                assert_eq!(name, "get_weather");
                assert_eq!(response["temp"], 72);
            }
            other => panic!("expected FunctionResponse, got {other:?}"),
        }
    }

    #[test]
    fn gemini_generation_config_serde_roundtrip() {
        let cfg = GeminiGenerationConfig {
            max_output_tokens: Some(1024),
            temperature: Some(0.5),
            top_p: Some(0.8),
            top_k: Some(20),
            candidate_count: None,
            stop_sequences: None,
            response_mime_type: None,
            response_schema: None,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let parsed: GeminiGenerationConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.max_output_tokens, Some(1024));
        assert_eq!(parsed.temperature, Some(0.5));
    }

    #[test]
    fn harm_category_serde_roundtrip() {
        for cat in [
            HarmCategory::HarmCategoryHarassment,
            HarmCategory::HarmCategoryHateSpeech,
            HarmCategory::HarmCategorySexuallyExplicit,
            HarmCategory::HarmCategoryDangerousContent,
            HarmCategory::HarmCategoryCivicIntegrity,
        ] {
            let json = serde_json::to_string(&cat).unwrap();
            let parsed: HarmCategory = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, cat);
        }
    }

    #[test]
    fn harm_block_threshold_serde_roundtrip() {
        for threshold in [
            HarmBlockThreshold::BlockNone,
            HarmBlockThreshold::BlockLowAndAbove,
            HarmBlockThreshold::BlockMediumAndAbove,
            HarmBlockThreshold::BlockOnlyHigh,
        ] {
            let json = serde_json::to_string(&threshold).unwrap();
            let parsed: HarmBlockThreshold = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, threshold);
        }
    }

    #[test]
    fn gemini_safety_setting_serde() {
        let ss = GeminiSafetySetting {
            category: HarmCategory::HarmCategoryHateSpeech,
            threshold: HarmBlockThreshold::BlockOnlyHigh,
        };
        let json = serde_json::to_string(&ss).unwrap();
        let parsed: GeminiSafetySetting = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ss);
    }

    #[test]
    fn gemini_tool_serde_roundtrip() {
        let tool = GeminiTool {
            function_declarations: vec![GeminiFunctionDeclaration {
                name: "calc".into(),
                description: "Calculate".into(),
                parameters: json!({"type": "object", "properties": {"expr": {"type": "string"}}}),
            }],
        };
        let json = serde_json::to_string(&tool).unwrap();
        let parsed: GeminiTool = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, tool);
    }

    #[test]
    fn function_calling_mode_serde() {
        for (mode, expected) in [
            (FunctionCallingMode::Auto, r#""AUTO""#),
            (FunctionCallingMode::Any, r#""ANY""#),
            (FunctionCallingMode::None, r#""NONE""#),
        ] {
            let json = serde_json::to_string(&mode).unwrap();
            assert_eq!(json, expected);
        }
    }

    #[test]
    fn gemini_response_serde_roundtrip() {
        let resp = GeminiResponse {
            candidates: vec![GeminiCandidate {
                content: GeminiContent {
                    role: "model".into(),
                    parts: vec![GeminiPart::Text("Rust is great.".into())],
                },
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
                citation_metadata: None,
            }],
            prompt_feedback: None,
            usage_metadata: Some(GeminiUsageMetadata {
                prompt_token_count: 10,
                candidates_token_count: 20,
                total_token_count: 30,
            }),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: GeminiResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.candidates.len(), 1);
    }

    #[test]
    fn gemini_stream_chunk_serde_roundtrip() {
        let chunk = GeminiStreamChunk {
            candidates: vec![GeminiCandidate {
                content: GeminiContent {
                    role: "model".into(),
                    parts: vec![GeminiPart::Text("Hello".into())],
                },
                finish_reason: None,
                safety_ratings: None,
                citation_metadata: None,
            }],
            usage_metadata: None,
        };
        let json = serde_json::to_string(&chunk).unwrap();
        let parsed: GeminiStreamChunk = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.candidates.len(), 1);
    }

    // -- From<GeminiRequest> for WorkOrder --

    #[test]
    fn request_to_work_order_extracts_task() {
        let req = GeminiRequest {
            model: "gemini-2.5-flash".into(),
            contents: vec![GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::Text("Explain traits".into())],
            }],
            system_instruction: None,
            generation_config: None,
            safety_settings: None,
            tools: None,
            tool_config: None,
        };
        let wo: WorkOrder = req.into();
        assert_eq!(wo.task, "Explain traits");
        assert_eq!(wo.config.model.as_deref(), Some("gemini-2.5-flash"));
    }

    #[test]
    fn request_to_work_order_stores_system_instruction_as_snippet() {
        let req = GeminiRequest {
            model: "gemini-2.5-flash".into(),
            contents: vec![GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::Text("Hi".into())],
            }],
            system_instruction: Some(GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::Text("Be helpful.".into())],
            }),
            generation_config: None,
            safety_settings: None,
            tools: None,
            tool_config: None,
        };
        let wo: WorkOrder = req.into();
        assert!(!wo.context.snippets.is_empty());
        assert_eq!(wo.context.snippets[0].content, "Be helpful.");
    }

    #[test]
    fn request_to_work_order_stores_tools_in_vendor() {
        let req = GeminiRequest {
            model: "gemini-2.5-flash".into(),
            contents: vec![GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::Text("Go".into())],
            }],
            system_instruction: None,
            generation_config: None,
            safety_settings: None,
            tools: Some(vec![GeminiTool {
                function_declarations: vec![GeminiFunctionDeclaration {
                    name: "search".into(),
                    description: "Search".into(),
                    parameters: json!({}),
                }],
            }]),
            tool_config: None,
        };
        let wo: WorkOrder = req.into();
        assert!(wo.config.vendor.contains_key("tools"));
    }

    #[test]
    fn request_to_work_order_stores_safety_settings_in_vendor() {
        let req = GeminiRequest {
            model: "gemini-2.5-flash".into(),
            contents: vec![GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::Text("Go".into())],
            }],
            system_instruction: None,
            generation_config: None,
            safety_settings: Some(vec![GeminiSafetySetting {
                category: HarmCategory::HarmCategoryHarassment,
                threshold: HarmBlockThreshold::BlockNone,
            }]),
            tools: None,
            tool_config: None,
        };
        let wo: WorkOrder = req.into();
        assert!(wo.config.vendor.contains_key("safety_settings"));
    }

    // -- From<Receipt> for GeminiResponse --

    #[test]
    fn receipt_to_response_maps_text_parts() {
        let receipt = make_receipt(
            "google/gemini-2.5-flash",
            vec![assistant_event("Hello from Gemini!")],
            usage(50, 30),
            Outcome::Complete,
        );
        let resp: GeminiResponse = receipt.into();
        assert_eq!(resp.candidates.len(), 1);
        let parts = &resp.candidates[0].content.parts;
        assert!(matches!(&parts[0], GeminiPart::Text(t) if t == "Hello from Gemini!"));
        assert_eq!(resp.candidates[0].finish_reason.as_deref(), Some("STOP"));
    }

    #[test]
    fn receipt_to_response_maps_function_calls() {
        let receipt = make_receipt(
            "google/gemini-2.5-flash",
            vec![
                tool_call_event("search", None, json!({"q": "rust"})),
                tool_result_event("search", None, json!({"results": []})),
            ],
            UsageNormalized::default(),
            Outcome::Complete,
        );
        let resp: GeminiResponse = receipt.into();
        let parts = &resp.candidates[0].content.parts;
        assert_eq!(parts.len(), 2);
        assert!(matches!(&parts[0], GeminiPart::FunctionCall { name, .. } if name == "search"));
        assert!(matches!(&parts[1], GeminiPart::FunctionResponse { name, .. } if name == "search"));
    }

    #[test]
    fn receipt_to_response_maps_usage_metadata() {
        let receipt = make_receipt(
            "google/gemini-2.5-flash",
            vec![],
            usage(100, 50),
            Outcome::Complete,
        );
        let resp: GeminiResponse = receipt.into();
        let um = resp.usage_metadata.unwrap();
        assert_eq!(um.prompt_token_count, 100);
        assert_eq!(um.candidates_token_count, 50);
        assert_eq!(um.total_token_count, 150);
    }
}

// ===========================================================================
// 4. Kimi SDK (abp_kimi_sdk::api) — 15 tests
// ===========================================================================

mod kimi {
    use super::*;
    use abp_kimi_sdk::api::*;

    fn make_kimi_request(messages: Vec<KimiMessage>) -> KimiChatRequest {
        KimiChatRequest {
            model: "moonshot-v1-8k".into(),
            messages,
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
        }
    }

    // -- Serde roundtrips --

    #[test]
    fn kimi_system_message_serde() {
        let msg = KimiMessage::System {
            content: "You are Kimi.".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""role":"system""#));
        let parsed: KimiMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, msg);
    }

    #[test]
    fn kimi_user_message_serde() {
        let msg = KimiMessage::User {
            content: "Hello".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""role":"user""#));
        let parsed: KimiMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, msg);
    }

    #[test]
    fn kimi_assistant_message_with_tool_calls_serde() {
        let msg = KimiMessage::Assistant {
            content: None,
            tool_calls: Some(vec![KimiToolCall {
                id: "call_1".into(),
                call_type: "function".into(),
                function: KimiFunctionCall {
                    name: "search".into(),
                    arguments: r#"{"q":"rust"}"#.into(),
                },
            }]),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: KimiMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, msg);
    }

    #[test]
    fn kimi_tool_message_serde() {
        let msg = KimiMessage::Tool {
            tool_call_id: "call_1".into(),
            content: "result data".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""role":"tool""#));
        let parsed: KimiMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, msg);
    }

    #[test]
    fn kimi_chat_request_full_serde_roundtrip() {
        let req = KimiChatRequest {
            model: "moonshot-v1-128k".into(),
            messages: vec![
                KimiMessage::System {
                    content: "Be brief.".into(),
                },
                KimiMessage::User {
                    content: "Summarize".into(),
                },
            ],
            temperature: Some(0.3),
            top_p: Some(0.8),
            max_tokens: Some(1024),
            tools: Some(vec![KimiToolDef {
                tool_type: "function".into(),
                function: KimiFunctionDef {
                    name: "web_search".into(),
                    description: Some("Search the web".into()),
                    parameters: Some(json!({"type": "object"})),
                },
            }]),
            tool_choice: Some(json!("auto")),
            stream: Some(true),
            n: Some(1),
            stop: Some(vec!["STOP".into()]),
            presence_penalty: Some(0.1),
            frequency_penalty: Some(0.2),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: KimiChatRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, req);
    }

    #[test]
    fn kimi_finish_reason_variants_serde() {
        for (variant, expected) in [
            (KimiFinishReason::Stop, r#""stop""#),
            (KimiFinishReason::Length, r#""length""#),
            (KimiFinishReason::ToolCalls, r#""tool_calls""#),
            (KimiFinishReason::ContentFilter, r#""content_filter""#),
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, expected);
        }
    }

    #[test]
    fn kimi_stream_chunk_serde_roundtrip() {
        let chunk = KimiStreamChunk {
            id: "kimi-chunk-1".into(),
            object: "chat.completion.chunk".into(),
            created: 1700000000,
            model: "moonshot-v1-8k".into(),
            choices: vec![KimiStreamChoice {
                index: 0,
                delta: KimiDelta {
                    role: Some("assistant".into()),
                    content: Some("Hi".into()),
                    tool_calls: None,
                },
                finish_reason: None,
                safety_ratings: None,
            }],
            usage: None,
        };
        let json = serde_json::to_string(&chunk).unwrap();
        let parsed: KimiStreamChunk = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, chunk);
    }

    #[test]
    fn kimi_chat_response_serde_roundtrip() {
        let resp = KimiChatResponse {
            id: "cmpl-test".into(),
            object: "chat.completion".into(),
            created: 1700000000,
            model: "moonshot-v1-8k".into(),
            choices: vec![KimiChoice {
                index: 0,
                message: KimiAssistantMessage {
                    role: "assistant".into(),
                    content: Some("Done!".into()),
                    tool_calls: None,
                },
                finish_reason: KimiFinishReason::Stop,
                safety_ratings: None,
            }],
            usage: Some(KimiUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            }),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: KimiChatResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, resp);
    }

    // -- From<KimiChatRequest> for WorkOrder --

    #[test]
    fn request_to_work_order_extracts_last_user_message() {
        let req = make_kimi_request(vec![
            KimiMessage::User {
                content: "First".into(),
            },
            KimiMessage::User {
                content: "Second".into(),
            },
        ]);
        let wo: WorkOrder = req.into();
        assert_eq!(wo.task, "Second");
    }

    #[test]
    fn request_to_work_order_maps_system_to_snippets() {
        let req = make_kimi_request(vec![
            KimiMessage::System {
                content: "Be brief.".into(),
            },
            KimiMessage::User {
                content: "Hi".into(),
            },
        ]);
        let wo: WorkOrder = req.into();
        assert_eq!(wo.context.snippets.len(), 1);
        assert_eq!(wo.context.snippets[0].content, "Be brief.");
    }

    #[test]
    fn request_to_work_order_preserves_model() {
        let req = make_kimi_request(vec![KimiMessage::User {
            content: "Test".into(),
        }]);
        let wo: WorkOrder = req.into();
        assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-8k"));
    }

    // -- From<Receipt> for KimiChatResponse --

    #[test]
    fn receipt_to_response_maps_assistant_text() {
        let receipt = make_receipt(
            "kimi/moonshot-v1-8k",
            vec![assistant_event("Kimi says hello!")],
            usage(80, 40),
            Outcome::Complete,
        );
        let resp: KimiChatResponse = receipt.into();
        assert_eq!(resp.object, "chat.completion");
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Kimi says hello!")
        );
        assert_eq!(resp.choices[0].finish_reason, KimiFinishReason::Stop);
    }

    #[test]
    fn receipt_to_response_maps_tool_calls() {
        let receipt = make_receipt(
            "kimi/moonshot-v1-8k",
            vec![tool_call_event(
                "search",
                Some("call_k1"),
                json!({"q": "test"}),
            )],
            UsageNormalized::default(),
            Outcome::Complete,
        );
        let resp: KimiChatResponse = receipt.into();
        assert_eq!(resp.choices[0].finish_reason, KimiFinishReason::ToolCalls);
        let tc = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tc[0].id, "call_k1");
        assert_eq!(tc[0].function.name, "search");
    }

    #[test]
    fn receipt_to_response_maps_usage() {
        let receipt = make_receipt(
            "kimi/moonshot-v1-8k",
            vec![],
            usage(200, 100),
            Outcome::Complete,
        );
        let resp: KimiChatResponse = receipt.into();
        let u = resp.usage.unwrap();
        assert_eq!(u.prompt_tokens, 200);
        assert_eq!(u.completion_tokens, 100);
        assert_eq!(u.total_tokens, 300);
    }

    #[test]
    fn receipt_to_response_id_format() {
        let receipt = make_receipt(
            "kimi/moonshot-v1-8k",
            vec![],
            UsageNormalized::default(),
            Outcome::Complete,
        );
        let run_id = receipt.meta.run_id;
        let resp: KimiChatResponse = receipt.into();
        assert!(resp.id.starts_with("cmpl-"));
        assert!(resp.id.contains(&run_id.to_string()));
    }
}

// ===========================================================================
// 5. Codex SDK (abp_codex_sdk::api) — 15 tests
// ===========================================================================

mod codex {
    use super::*;
    use abp_codex_sdk::api::*;

    fn make_codex_request(input: Vec<CodexInputItem>) -> CodexSessionRequest {
        CodexSessionRequest {
            model: "codex-mini-latest".into(),
            instructions: None,
            input,
            tools: None,
            stream: None,
            previous_response_id: None,
            max_output_tokens: None,
            temperature: None,
        }
    }

    // -- Serde roundtrips --

    #[test]
    fn input_item_message_serde_roundtrip() {
        let item = CodexInputItem::Message {
            role: "user".into(),
            content: "Fix the bug".into(),
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains(r#""type":"message""#));
        let parsed: CodexInputItem = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, item);
    }

    #[test]
    fn input_item_function_call_output_serde_roundtrip() {
        let item = CodexInputItem::FunctionCallOutput {
            call_id: "call_xyz".into(),
            output: "result data".into(),
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains(r#""type":"function_call_output""#));
        let parsed: CodexInputItem = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, item);
    }

    #[test]
    fn output_item_message_serde_roundtrip() {
        let item = CodexOutputItem::Message {
            role: "assistant".into(),
            content: vec![CodexContentPart::OutputText {
                text: "Done!".into(),
            }],
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains(r#""type":"message""#));
        let parsed: CodexOutputItem = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, item);
    }

    #[test]
    fn output_item_function_call_serde_roundtrip() {
        let item = CodexOutputItem::FunctionCall {
            id: "fc_1".into(),
            call_id: Some("corr_1".into()),
            name: "shell".into(),
            arguments: r#"{"command":"ls"}"#.into(),
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains(r#""type":"function_call""#));
        let parsed: CodexOutputItem = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, item);
    }

    #[test]
    fn session_request_full_serde_roundtrip() {
        let req = CodexSessionRequest {
            model: "codex-mini-latest".into(),
            instructions: Some("Be helpful.".into()),
            input: vec![CodexInputItem::Message {
                role: "user".into(),
                content: "Fix the bug".into(),
            }],
            tools: Some(vec![CodexTool {
                tool_type: "function".into(),
                function: CodexFunctionDef {
                    name: "bash".into(),
                    description: Some("Run command".into()),
                    parameters: Some(json!({"type": "object"})),
                },
            }]),
            stream: Some(true),
            previous_response_id: Some("resp_prev".into()),
            max_output_tokens: Some(4096),
            temperature: Some(0.7),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: CodexSessionRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, req);
    }

    #[test]
    fn session_request_omits_none_fields() {
        let req = make_codex_request(vec![]);
        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.contains("instructions"));
        assert!(!json.contains("tools"));
        assert!(!json.contains("stream"));
        assert!(!json.contains("previous_response_id"));
    }

    #[test]
    fn session_response_serde_roundtrip() {
        let resp = CodexSessionResponse {
            id: "resp_abc".into(),
            object: "response".into(),
            status: "completed".into(),
            output: vec![CodexOutputItem::Message {
                role: "assistant".into(),
                content: vec![CodexContentPart::OutputText {
                    text: "All done!".into(),
                }],
            }],
            usage: Some(CodexUsage {
                input_tokens: 100,
                output_tokens: 50,
                total_tokens: 150,
            }),
            model: "codex-mini-latest".into(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: CodexSessionResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, resp);
    }

    // -- From<CodexSessionRequest> for WorkOrder --

    #[test]
    fn request_to_work_order_uses_last_user_message() {
        let req = make_codex_request(vec![
            CodexInputItem::Message {
                role: "user".into(),
                content: "First".into(),
            },
            CodexInputItem::Message {
                role: "user".into(),
                content: "Second".into(),
            },
        ]);
        let wo: WorkOrder = req.into();
        assert_eq!(wo.task, "Second");
    }

    #[test]
    fn request_to_work_order_maps_instructions_to_snippet() {
        let mut req = make_codex_request(vec![CodexInputItem::Message {
            role: "user".into(),
            content: "Fix it".into(),
        }]);
        req.instructions = Some("Be concise.".into());
        let wo: WorkOrder = req.into();
        assert_eq!(wo.context.snippets.len(), 1);
        assert_eq!(wo.context.snippets[0].name, "instructions");
    }

    #[test]
    fn request_to_work_order_preserves_model() {
        let mut req = make_codex_request(vec![CodexInputItem::Message {
            role: "user".into(),
            content: "Go".into(),
        }]);
        req.model = "o4-mini".into();
        let wo: WorkOrder = req.into();
        assert_eq!(wo.config.model.as_deref(), Some("o4-mini"));
    }

    #[test]
    fn request_to_work_order_previous_response_id_in_vendor() {
        let mut req = make_codex_request(vec![CodexInputItem::Message {
            role: "user".into(),
            content: "Continue".into(),
        }]);
        req.previous_response_id = Some("resp_prev_123".into());
        let wo: WorkOrder = req.into();
        assert_eq!(
            wo.config.vendor.get("previous_response_id"),
            Some(&serde_json::Value::String("resp_prev_123".into()))
        );
    }

    // -- From<Receipt> for CodexSessionResponse --

    #[test]
    fn receipt_to_response_maps_assistant_text() {
        let receipt = make_receipt(
            "openai/codex-mini-latest",
            vec![assistant_event("Hello!")],
            UsageNormalized::default(),
            Outcome::Complete,
        );
        let resp: CodexSessionResponse = receipt.into();
        assert_eq!(resp.object, "response");
        assert_eq!(resp.status, "completed");
        assert_eq!(resp.output.len(), 1);
        match &resp.output[0] {
            CodexOutputItem::Message { role, content } => {
                assert_eq!(role, "assistant");
                match &content[0] {
                    CodexContentPart::OutputText { text } => assert_eq!(text, "Hello!"),
                }
            }
            other => panic!("expected Message, got {other:?}"),
        }
    }

    #[test]
    fn receipt_to_response_maps_tool_calls() {
        let receipt = make_receipt(
            "openai/codex-mini-latest",
            vec![tool_call_event(
                "read_file",
                Some("call_abc"),
                json!({"path": "main.rs"}),
            )],
            UsageNormalized::default(),
            Outcome::Complete,
        );
        let resp: CodexSessionResponse = receipt.into();
        assert_eq!(resp.output.len(), 1);
        match &resp.output[0] {
            CodexOutputItem::FunctionCall { id, name, .. } => {
                assert_eq!(id, "call_abc");
                assert_eq!(name, "read_file");
            }
            other => panic!("expected FunctionCall, got {other:?}"),
        }
    }

    #[test]
    fn receipt_to_response_failed_outcome() {
        let receipt = make_receipt(
            "openai/codex-mini-latest",
            vec![],
            UsageNormalized::default(),
            Outcome::Failed,
        );
        let resp: CodexSessionResponse = receipt.into();
        assert_eq!(resp.status, "failed");
    }
}

// ===========================================================================
// 6. Copilot SDK (abp_copilot_sdk::api) — 15 tests
// ===========================================================================

mod copilot {
    use super::*;
    use abp_copilot_sdk::api::*;
    use abp_copilot_sdk::dialect::{CopilotConfirmation, CopilotReference, CopilotReferenceType};

    fn make_copilot_request(messages: Vec<CopilotMessage>) -> CopilotRequest {
        CopilotRequest {
            model: "gpt-4o".into(),
            messages,
            stream: None,
            temperature: None,
            max_tokens: None,
            references: vec![],
            copilot_metadata: None,
        }
    }

    // -- Serde roundtrips --

    #[test]
    fn copilot_message_user_serde_roundtrip() {
        let msg = CopilotMessage {
            role: "user".into(),
            content: "Help me!".into(),
            name: None,
            function_call: None,
            copilot_references: vec![],
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: CopilotMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, msg);
    }

    #[test]
    fn copilot_message_with_references_serde_roundtrip() {
        let msg = CopilotMessage {
            role: "user".into(),
            content: "Check this file".into(),
            name: None,
            function_call: None,
            copilot_references: vec![CopilotReference {
                ref_type: CopilotReferenceType::File,
                id: "file_1".into(),
                data: json!({"path": "src/main.rs", "content": "fn main() {}"}),
                metadata: None,
            }],
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("copilot_references"));
        let parsed: CopilotMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, msg);
    }

    #[test]
    fn copilot_message_with_function_call_serde_roundtrip() {
        let msg = CopilotMessage {
            role: "assistant".into(),
            content: "".into(),
            name: Some("copilot".into()),
            function_call: Some(CopilotFunctionCall {
                name: "run_command".into(),
                arguments: r#"{"cmd":"cargo test"}"#.into(),
            }),
            copilot_references: vec![],
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: CopilotMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, msg);
    }

    #[test]
    fn copilot_request_full_serde_roundtrip() {
        let req = CopilotRequest {
            model: "gpt-4o".into(),
            messages: vec![
                CopilotMessage {
                    role: "system".into(),
                    content: "You are a coding assistant.".into(),
                    name: None,
                    function_call: None,
                    copilot_references: vec![],
                },
                CopilotMessage {
                    role: "user".into(),
                    content: "Fix the bug in main.rs".into(),
                    name: None,
                    function_call: None,
                    copilot_references: vec![],
                },
            ],
            stream: Some(true),
            temperature: Some(0.5),
            max_tokens: Some(2048),
            references: vec![CopilotReference {
                ref_type: CopilotReferenceType::Repository,
                id: "repo_1".into(),
                data: json!({"owner": "test", "name": "repo"}),
                metadata: None,
            }],
            copilot_metadata: Some({
                let mut m = BTreeMap::new();
                m.insert("agent".into(), json!("copilot-extension"));
                m
            }),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: CopilotRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, req);
    }

    #[test]
    fn copilot_reference_types_serde() {
        for rt in [
            CopilotReferenceType::File,
            CopilotReferenceType::Snippet,
            CopilotReferenceType::Repository,
            CopilotReferenceType::WebSearchResult,
        ] {
            let json = serde_json::to_string(&rt).unwrap();
            let parsed: CopilotReferenceType = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, rt);
        }
    }

    #[test]
    fn copilot_finish_reason_variants_serde() {
        for (variant, expected) in [
            (CopilotFinishReason::Stop, r#""stop""#),
            (CopilotFinishReason::Length, r#""length""#),
            (CopilotFinishReason::FunctionCall, r#""function_call""#),
            (CopilotFinishReason::ContentFilter, r#""content_filter""#),
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, expected);
        }
    }

    #[test]
    fn copilot_response_serde_roundtrip() {
        let resp = CopilotResponse {
            id: "copilot-test".into(),
            object: "chat.completion".into(),
            created: 1700000000,
            model: "gpt-4o".into(),
            choices: vec![CopilotChoice {
                index: 0,
                message: CopilotAssistantMessage {
                    role: "assistant".into(),
                    content: Some("Hello!".into()),
                    function_call: None,
                    copilot_references: vec![],
                },
                finish_reason: CopilotFinishReason::Stop,
                safety_ratings: None,
            }],
            usage: Some(CopilotUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            }),
            copilot_confirmation: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: CopilotResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, resp);
    }

    #[test]
    fn copilot_confirmation_serde_roundtrip() {
        let conf = CopilotConfirmation {
            id: "conf_1".into(),
            title: "Delete file?".into(),
            message: "Are you sure you want to delete main.rs?".into(),
            accepted: Some(true),
        };
        let json = serde_json::to_string(&conf).unwrap();
        let parsed: CopilotConfirmation = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, conf);
    }

    // -- From<CopilotRequest> for WorkOrder --

    #[test]
    fn request_to_work_order_extracts_last_user_message() {
        let req = make_copilot_request(vec![
            CopilotMessage {
                role: "user".into(),
                content: "First".into(),
                name: None,
                function_call: None,
                copilot_references: vec![],
            },
            CopilotMessage {
                role: "user".into(),
                content: "Second".into(),
                name: None,
                function_call: None,
                copilot_references: vec![],
            },
        ]);
        let wo: WorkOrder = req.into();
        assert_eq!(wo.task, "Second");
    }

    #[test]
    fn request_to_work_order_maps_system_to_snippets() {
        let req = make_copilot_request(vec![
            CopilotMessage {
                role: "system".into(),
                content: "System prompt.".into(),
                name: None,
                function_call: None,
                copilot_references: vec![],
            },
            CopilotMessage {
                role: "user".into(),
                content: "Hi".into(),
                name: None,
                function_call: None,
                copilot_references: vec![],
            },
        ]);
        let wo: WorkOrder = req.into();
        assert_eq!(wo.context.snippets.len(), 1);
        assert_eq!(wo.context.snippets[0].content, "System prompt.");
    }

    #[test]
    fn request_to_work_order_file_references_to_context_files() {
        let req = CopilotRequest {
            model: "gpt-4o".into(),
            messages: vec![CopilotMessage {
                role: "user".into(),
                content: "Check".into(),
                name: None,
                function_call: None,
                copilot_references: vec![],
            }],
            stream: None,
            temperature: None,
            max_tokens: None,
            references: vec![CopilotReference {
                ref_type: CopilotReferenceType::File,
                id: "file_1".into(),
                data: json!({"path": "src/main.rs"}),
                metadata: None,
            }],
            copilot_metadata: None,
        };
        let wo: WorkOrder = req.into();
        assert!(wo.context.files.contains(&"src/main.rs".to_string()));
    }

    // -- From<Receipt> for CopilotResponse --

    #[test]
    fn receipt_to_response_maps_assistant_text() {
        let receipt = make_receipt(
            "copilot/gpt-4o",
            vec![assistant_event("Hello from Copilot!")],
            usage(100, 50),
            Outcome::Complete,
        );
        let resp: CopilotResponse = receipt.into();
        assert_eq!(resp.object, "chat.completion");
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Hello from Copilot!")
        );
        assert_eq!(resp.choices[0].finish_reason, CopilotFinishReason::Stop);
    }

    #[test]
    fn receipt_to_response_maps_function_call() {
        let receipt = make_receipt(
            "copilot/gpt-4o",
            vec![tool_call_event("run_cmd", None, json!({"cmd": "ls"}))],
            UsageNormalized::default(),
            Outcome::Complete,
        );
        let resp: CopilotResponse = receipt.into();
        assert_eq!(
            resp.choices[0].finish_reason,
            CopilotFinishReason::FunctionCall
        );
        let fc = resp.choices[0].message.function_call.as_ref().unwrap();
        assert_eq!(fc.name, "run_cmd");
    }

    #[test]
    fn receipt_to_response_maps_usage() {
        let receipt = make_receipt("copilot/gpt-4o", vec![], usage(300, 150), Outcome::Complete);
        let resp: CopilotResponse = receipt.into();
        let u = resp.usage.unwrap();
        assert_eq!(u.prompt_tokens, 300);
        assert_eq!(u.completion_tokens, 150);
        assert_eq!(u.total_tokens, 450);
    }
}

// ===========================================================================
// Cross-SDK invariants — 5 bonus tests
// ===========================================================================

mod cross_sdk {
    use super::*;

    #[test]
    fn all_sdks_empty_input_yields_empty_task() {
        // OpenAI
        let oai_req = abp_openai_sdk::api::ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![],
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
            stream_options: None,
            parallel_tool_calls: None,
            service_tier: None,
        };
        let wo: WorkOrder = oai_req.into();
        assert_eq!(wo.task, "");

        // Codex
        let codex_req = abp_codex_sdk::api::CodexSessionRequest {
            model: "codex-mini-latest".into(),
            instructions: None,
            input: vec![],
            tools: None,
            stream: None,
            previous_response_id: None,
            max_output_tokens: None,
            temperature: None,
        };
        let wo: WorkOrder = codex_req.into();
        assert_eq!(wo.task, "");

        // Kimi
        let kimi_req = abp_kimi_sdk::api::KimiChatRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![],
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
        let wo: WorkOrder = kimi_req.into();
        assert_eq!(wo.task, "");

        // Copilot
        let copilot_req = abp_copilot_sdk::api::CopilotRequest {
            model: "gpt-4o".into(),
            messages: vec![],
            stream: None,
            temperature: None,
            max_tokens: None,
            references: vec![],
            copilot_metadata: None,
        };
        let wo: WorkOrder = copilot_req.into();
        assert_eq!(wo.task, "");
    }

    #[test]
    fn all_sdks_receipt_with_no_usage_omits_usage() {
        let receipt = make_receipt(
            "test/backend",
            vec![],
            UsageNormalized::default(),
            Outcome::Complete,
        );

        let oai: abp_openai_sdk::api::ChatCompletionResponse = receipt.clone().into();
        assert!(oai.usage.is_none());

        let kimi: abp_kimi_sdk::api::KimiChatResponse = receipt.clone().into();
        assert!(kimi.usage.is_none());

        let codex: abp_codex_sdk::api::CodexSessionResponse = receipt.clone().into();
        assert!(codex.usage.is_none());

        let copilot: abp_copilot_sdk::api::CopilotResponse = receipt.into();
        assert!(copilot.usage.is_none());
    }

    #[test]
    fn all_sdks_failed_receipt_maps_correctly() {
        let receipt = make_receipt(
            "test/backend",
            vec![error_event("something went wrong")],
            UsageNormalized::default(),
            Outcome::Failed,
        );

        let oai: abp_openai_sdk::api::ChatCompletionResponse = receipt.clone().into();
        assert_eq!(
            oai.choices[0].finish_reason,
            abp_openai_sdk::api::FinishReason::Stop
        );

        let kimi: abp_kimi_sdk::api::KimiChatResponse = receipt.clone().into();
        assert_eq!(
            kimi.choices[0].finish_reason,
            abp_kimi_sdk::api::KimiFinishReason::Stop
        );

        let codex: abp_codex_sdk::api::CodexSessionResponse = receipt.clone().into();
        assert_eq!(codex.status, "failed");

        let copilot: abp_copilot_sdk::api::CopilotResponse = receipt.into();
        assert_eq!(
            copilot.choices[0].finish_reason,
            abp_copilot_sdk::api::CopilotFinishReason::Stop
        );
    }

    #[test]
    fn all_sdks_partial_receipt_maps_correctly() {
        let receipt = make_receipt(
            "test/backend",
            vec![],
            UsageNormalized::default(),
            Outcome::Partial,
        );

        let codex: abp_codex_sdk::api::CodexSessionResponse = receipt.clone().into();
        assert_eq!(codex.status, "incomplete");

        let claude: abp_claude_sdk::messages::MessagesResponse = receipt.into();
        assert_eq!(claude.stop_reason.as_deref(), Some("max_tokens"));
    }

    #[test]
    fn all_sdks_concatenate_multiple_assistant_messages() {
        let trace = vec![assistant_event("Part 1. "), assistant_event("Part 2.")];
        let receipt = make_receipt(
            "test/backend",
            trace,
            UsageNormalized::default(),
            Outcome::Complete,
        );

        let oai: abp_openai_sdk::api::ChatCompletionResponse = receipt.clone().into();
        assert_eq!(
            oai.choices[0].message.content.as_deref(),
            Some("Part 1. Part 2.")
        );

        let kimi: abp_kimi_sdk::api::KimiChatResponse = receipt.clone().into();
        assert_eq!(
            kimi.choices[0].message.content.as_deref(),
            Some("Part 1. Part 2.")
        );

        let copilot: abp_copilot_sdk::api::CopilotResponse = receipt.into();
        assert_eq!(
            copilot.choices[0].message.content.as_deref(),
            Some("Part 1. Part 2.")
        );
    }
}
