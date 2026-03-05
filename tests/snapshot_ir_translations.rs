#![allow(dead_code, unused_imports)]
#![allow(clippy::all)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Exhaustive snapshot tests for IR translations, protocol envelopes,
//! capability negotiation, error taxonomy, projection matrix, and streaming.
//!
//! 55 tests across 12 sections covering all bridge crates and core subsystems.

use serde_json::json;
use std::collections::BTreeMap;

use chrono::{TimeZone, Utc};
use uuid::Uuid;

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ExecutionMode, MinSupport, Outcome, Receipt,
    RunMetadata, SupportLevel, UsageNormalized, VerificationReport, WorkOrderBuilder,
};
use abp_sdk_types::ir::{IrContentPart, IrMessage, IrRole, IrToolCall, IrToolDefinition, IrUsage};
use abp_sdk_types::ir_request::{IrChatRequest, IrSamplingParams, IrStreamConfig};
use abp_sdk_types::ir_response::{IrChatResponse, IrChoice, IrFinishReason, IrStreamChunk};

use abp_dialect::ir::{
    IrContentBlock, IrGenerationConfig, IrMessage as DialectIrMessage,
    IrRequest as DialectIrRequest, IrResponse as DialectIrResponse, IrRole as DialectIrRole,
    IrStopReason, IrStreamEvent, IrToolDefinition as DialectIrToolDef, IrUsage as DialectIrUsage,
};

// ── Helpers ─────────────────────────────────────────────────────────────

fn fixed_ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap()
}

fn fixed_uuid() -> Uuid {
    Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap()
}

fn fixed_uuid2() -> Uuid {
    Uuid::parse_str("00000000-0000-4000-8000-000000000002").unwrap()
}

fn sample_capabilities() -> CapabilityManifest {
    let mut caps = BTreeMap::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolUse, SupportLevel::Native);
    caps.insert(Capability::ExtendedThinking, SupportLevel::Emulated);
    caps
}

fn sample_receipt() -> Receipt {
    Receipt {
        meta: RunMetadata {
            run_id: fixed_uuid(),
            work_order_id: fixed_uuid2(),
            contract_version: "abp/v0.1".into(),
            started_at: fixed_ts(),
            finished_at: fixed_ts(),
            duration_ms: 1234,
        },
        backend: BackendIdentity {
            id: "sidecar:test".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: Some("0.1.0".into()),
        },
        capabilities: sample_capabilities(),
        mode: ExecutionMode::Mapped,
        usage_raw: json!({"input_tokens": 100, "output_tokens": 50}),
        usage: UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            cache_read_tokens: None,
            cache_write_tokens: None,
            request_units: None,
            estimated_cost_usd: Some(0.001),
        },
        trace: vec![AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::RunStarted {
                message: "starting".into(),
            },
            ext: None,
        }],
        artifacts: vec![ArtifactRef {
            kind: "file".into(),
            path: "output.txt".into(),
        }],
        verification: VerificationReport {
            git_diff: Some("diff --git a/file.txt".into()),
            git_status: Some("M file.txt".into()),
            harness_ok: true,
        },
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. OpenAI bridge IR translations (8 tests)
// ═══════════════════════════════════════════════════════════════════════════

mod openai_ir {
    use super::*;
    use openai_bridge::ir_translate;
    use openai_bridge::openai_types::*;

    #[test]
    fn openai_minimal_request_to_ir() {
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![ChatMessage {
                role: ChatMessageRole::User,
                content: Some("Hello".into()),
                tool_calls: None,
                tool_call_id: None,
            }],
            tools: None,
            temperature: None,
            max_tokens: None,
            stream: None,
            top_p: None,
            frequency_penalty: None,
            presence_penalty: None,
            stop: None,
            n: None,
            tool_choice: None,
        };
        let ir = ir_translate::openai_request_to_ir(&req);
        insta::assert_json_snapshot!("openai_minimal_request_to_ir", &ir);
    }

    #[test]
    fn openai_full_request_to_ir() {
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![
                ChatMessage {
                    role: ChatMessageRole::System,
                    content: Some("You are helpful".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                ChatMessage {
                    role: ChatMessageRole::User,
                    content: Some("What is 2+2?".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            tools: Some(vec![ToolDefinition {
                tool_type: "function".into(),
                function: FunctionDefinition {
                    name: "calculator".into(),
                    description: "Evaluate math".into(),
                    parameters: json!({"type":"object","properties":{"expr":{"type":"string"}}}),
                },
            }]),
            temperature: Some(0.7),
            max_tokens: Some(2048),
            stream: Some(false),
            top_p: Some(0.95),
            frequency_penalty: Some(0.1),
            presence_penalty: Some(0.2),
            stop: Some(vec!["END".into()]),
            n: Some(1),
            tool_choice: Some(json!("auto")),
        };
        let ir = ir_translate::openai_request_to_ir(&req);
        insta::assert_json_snapshot!("openai_full_request_to_ir", &ir);
    }

    #[test]
    fn openai_response_to_ir() {
        let resp = ChatCompletionResponse {
            id: "chatcmpl-abc123".into(),
            object: "chat.completion".into(),
            created: 1700000000,
            model: "gpt-4o-2024-08-06".into(),
            choices: vec![ChatCompletionChoice {
                index: 0,
                message: ChatMessage {
                    role: ChatMessageRole::Assistant,
                    content: Some("4".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: Some(Usage {
                prompt_tokens: 25,
                completion_tokens: 1,
                total_tokens: 26,
            }),
        };
        let ir = ir_translate::openai_response_to_ir(&resp);
        insta::assert_json_snapshot!("openai_response_to_ir", &ir);
    }

    #[test]
    fn openai_tool_call_response_to_ir() {
        let resp = ChatCompletionResponse {
            id: "chatcmpl-tool".into(),
            object: "chat.completion".into(),
            created: 1700000000,
            model: "gpt-4o".into(),
            choices: vec![ChatCompletionChoice {
                index: 0,
                message: ChatMessage {
                    role: ChatMessageRole::Assistant,
                    content: None,
                    tool_calls: Some(vec![ToolCall {
                        id: "call_abc".into(),
                        call_type: "function".into(),
                        function: FunctionCall {
                            name: "get_weather".into(),
                            arguments: r#"{"city":"London"}"#.into(),
                        },
                    }]),
                    tool_call_id: None,
                },
                finish_reason: Some("tool_calls".into()),
            }],
            usage: Some(Usage {
                prompt_tokens: 50,
                completion_tokens: 20,
                total_tokens: 70,
            }),
        };
        let ir = ir_translate::openai_response_to_ir(&resp);
        insta::assert_json_snapshot!("openai_tool_call_response_to_ir", &ir);
    }

    #[test]
    fn openai_ir_roundtrip_request() {
        let ir = IrChatRequest::new("gpt-4o", vec![IrMessage::text(IrRole::User, "Hello world")])
            .with_max_tokens(512);
        let oai = ir_translate::ir_to_openai_request(&ir);
        let back = ir_translate::openai_request_to_ir(&oai);
        insta::assert_json_snapshot!("openai_ir_roundtrip_request", &back);
    }

    #[test]
    fn openai_ir_roundtrip_response() {
        let ir = IrChatResponse::text("Hello from the assistant")
            .with_id("resp-001")
            .with_model("gpt-4o")
            .with_usage(IrUsage::from_counts(10, 5));
        let oai = ir_translate::ir_to_openai_response(&ir);
        let back = ir_translate::openai_response_to_ir(&oai);
        insta::assert_json_snapshot!("openai_ir_roundtrip_response", &back);
    }

    #[test]
    fn openai_stream_chunk_to_ir() {
        let chunk = ChatCompletionChunk {
            id: "chatcmpl-stream".into(),
            object: "chat.completion.chunk".into(),
            created: 1700000000,
            model: "gpt-4o".into(),
            choices: vec![StreamChoice {
                index: 0,
                delta: StreamDelta {
                    role: Some("assistant".into()),
                    content: Some("Hello".into()),
                    tool_calls: None,
                },
                finish_reason: None,
            }],
        };
        let ir_chunks = ir_translate::openai_stream_to_ir(&chunk);
        insta::assert_json_snapshot!("openai_stream_chunk_to_ir", &ir_chunks);
    }

    #[test]
    fn openai_stream_final_chunk_to_ir() {
        let chunk = ChatCompletionChunk {
            id: "chatcmpl-stream".into(),
            object: "chat.completion.chunk".into(),
            created: 1700000000,
            model: "gpt-4o".into(),
            choices: vec![StreamChoice {
                index: 0,
                delta: StreamDelta {
                    role: None,
                    content: None,
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            }],
        };
        let ir_chunks = ir_translate::openai_stream_to_ir(&chunk);
        insta::assert_json_snapshot!("openai_stream_final_to_ir", &ir_chunks);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Claude bridge IR translations (8 tests)
// ═══════════════════════════════════════════════════════════════════════════

mod claude_ir {
    use super::*;
    use claude_bridge::claude_types::*;
    use claude_bridge::ir_translate;

    #[test]
    fn claude_minimal_request_to_ir() {
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
        let ir = ir_translate::claude_request_to_ir(&req);
        insta::assert_json_snapshot!("claude_minimal_request_to_ir", &ir);
    }

    #[test]
    fn claude_full_request_to_ir() {
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
                        id: "tool_01".into(),
                        name: "read_file".into(),
                        input: json!({"path": "src/main.rs"}),
                    }]),
                },
                Message {
                    role: Role::User,
                    content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                        tool_use_id: "tool_01".into(),
                        content: Some("fn main() {}".into()),
                        is_error: Some(false),
                    }]),
                },
            ],
            max_tokens: 4096,
            system: Some(SystemMessage::Text("You are a code reviewer.".into())),
            tools: Some(vec![ToolDefinition {
                name: "read_file".into(),
                description: "Read a file from disk".into(),
                input_schema: json!({"type":"object","properties":{"path":{"type":"string"}}}),
            }]),
            metadata: None,
            stream: Some(true),
            stop_sequences: Some(vec!["DONE".into()]),
            temperature: Some(0.5),
            top_p: Some(0.9),
            top_k: Some(40),
            tool_choice: Some(ToolChoice::Auto {}),
            thinking: None,
        };
        let ir = ir_translate::claude_request_to_ir(&req);
        insta::assert_json_snapshot!("claude_full_request_to_ir", &ir);
    }

    #[test]
    fn claude_response_to_ir() {
        let resp = MessagesResponse {
            id: "msg_abc123".into(),
            response_type: "message".into(),
            role: "assistant".into(),
            content: vec![ContentBlock::Text {
                text: "Here is my analysis.".into(),
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
        let ir = ir_translate::claude_response_to_ir(&resp);
        insta::assert_json_snapshot!("claude_response_to_ir", &ir);
    }

    #[test]
    fn claude_tool_use_response_to_ir() {
        let resp = MessagesResponse {
            id: "msg_tool".into(),
            response_type: "message".into(),
            role: "assistant".into(),
            content: vec![
                ContentBlock::Text {
                    text: "Let me read that file.".into(),
                },
                ContentBlock::ToolUse {
                    id: "toolu_01".into(),
                    name: "read_file".into(),
                    input: json!({"path": "/tmp/test.txt"}),
                },
            ],
            model: "claude-sonnet-4-20250514".into(),
            stop_reason: Some("tool_use".into()),
            stop_sequence: None,
            usage: Usage {
                input_tokens: 80,
                output_tokens: 30,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
        };
        let ir = ir_translate::claude_response_to_ir(&resp);
        insta::assert_json_snapshot!("claude_tool_use_response_to_ir", &ir);
    }

    #[test]
    fn claude_ir_roundtrip_request() {
        let ir = DialectIrRequest::new(vec![DialectIrMessage::text(
            DialectIrRole::User,
            "Explain Rust lifetimes",
        )])
        .with_model("claude-sonnet-4-20250514")
        .with_system_prompt("You are a Rust expert.");
        let claude = ir_translate::ir_to_claude_request(&ir);
        let back = ir_translate::claude_request_to_ir(&claude);
        insta::assert_json_snapshot!("claude_ir_roundtrip_request", &back);
    }

    #[test]
    fn claude_ir_roundtrip_response() {
        let ir = DialectIrResponse::text("Lifetimes ensure references are valid.")
            .with_id("msg_round")
            .with_model("claude-sonnet-4-20250514")
            .with_stop_reason(IrStopReason::EndTurn)
            .with_usage(DialectIrUsage::from_io(50, 25));
        let claude = ir_translate::ir_to_claude_response(&ir);
        let back = ir_translate::claude_response_to_ir(&claude);
        insta::assert_json_snapshot!("claude_ir_roundtrip_response", &back);
    }

    #[test]
    fn claude_stream_event_text_delta() {
        let event = StreamEvent::ContentBlockDelta {
            index: 0,
            delta: StreamDelta::TextDelta {
                text: "Hello ".into(),
            },
        };
        let ir_events = ir_translate::claude_stream_to_ir(&event);
        insta::assert_json_snapshot!("claude_stream_text_delta", &ir_events);
    }

    #[test]
    fn claude_stream_event_message_stop() {
        let event = StreamEvent::MessageStop {};
        let ir_events = ir_translate::claude_stream_to_ir(&event);
        insta::assert_json_snapshot!("claude_stream_message_stop", &ir_events);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Gemini bridge IR translations (6 tests)
// ═══════════════════════════════════════════════════════════════════════════

mod gemini_ir {
    use super::*;
    use gemini_bridge::gemini_types::*;
    use gemini_bridge::ir_translate;

    #[test]
    fn gemini_minimal_request_to_ir() {
        let req = GenerateContentRequest {
            model: "gemini-2.0-flash".into(),
            contents: vec![Content {
                role: Some("user".into()),
                parts: vec![Part::Text("Hello".into())],
            }],
            tools: None,
            generation_config: None,
            safety_settings: None,
            system_instruction: None,
        };
        let ir = ir_translate::gemini_request_to_ir(&req);
        insta::assert_json_snapshot!("gemini_minimal_request_to_ir", &ir);
    }

    #[test]
    fn gemini_full_request_to_ir() {
        let req = GenerateContentRequest {
            model: "gemini-2.0-flash".into(),
            contents: vec![
                Content {
                    role: Some("user".into()),
                    parts: vec![Part::Text("What is AI?".into())],
                },
                Content {
                    role: Some("model".into()),
                    parts: vec![Part::Text("AI is artificial intelligence.".into())],
                },
            ],
            tools: Some(vec![GeminiTool {
                function_declarations: vec![FunctionDeclaration {
                    name: "search".into(),
                    description: "Search the web".into(),
                    parameters: json!({"type":"object","properties":{"q":{"type":"string"}}}),
                }],
            }]),
            generation_config: Some(GenerationConfig {
                temperature: Some(0.8),
                top_p: Some(0.9),
                top_k: Some(32),
                max_output_tokens: Some(1024),
                candidate_count: None,
                stop_sequences: Some(vec!["STOP".into()]),
                response_mime_type: None,
                response_schema: None,
            }),
            safety_settings: None,
            system_instruction: Some(Content {
                role: Some("system".into()),
                parts: vec![Part::Text("Be concise.".into())],
            }),
        };
        let ir = ir_translate::gemini_request_to_ir(&req);
        insta::assert_json_snapshot!("gemini_full_request_to_ir", &ir);
    }

    #[test]
    fn gemini_response_to_ir() {
        let resp = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content {
                    role: Some("model".into()),
                    parts: vec![Part::Text("Artificial intelligence is...".into())],
                },
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
                citation_metadata: None,
            }],
            prompt_feedback: None,
            usage_metadata: Some(UsageMetadata {
                prompt_token_count: 30,
                candidates_token_count: 15,
                total_token_count: 45,
            }),
        };
        let ir = ir_translate::gemini_response_to_ir(&resp);
        insta::assert_json_snapshot!("gemini_response_to_ir", &ir);
    }

    #[test]
    fn gemini_ir_roundtrip_request() {
        let ir = IrChatRequest::new(
            "gemini-2.0-flash",
            vec![IrMessage::text(IrRole::User, "Summarize this")],
        )
        .with_max_tokens(256);
        let gemini = ir_translate::ir_to_gemini_request(&ir);
        let back = ir_translate::gemini_request_to_ir(&gemini);
        insta::assert_json_snapshot!("gemini_ir_roundtrip_request", &back);
    }

    #[test]
    fn gemini_stream_chunk_to_ir() {
        let chunk = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content {
                    role: Some("model".into()),
                    parts: vec![Part::Text("Partial ".into())],
                },
                finish_reason: None,
                safety_ratings: None,
                citation_metadata: None,
            }],
            prompt_feedback: None,
            usage_metadata: None,
        };
        let ir_chunks = ir_translate::gemini_stream_to_ir(&chunk);
        insta::assert_json_snapshot!("gemini_stream_chunk_to_ir", &ir_chunks);
    }

    #[test]
    fn gemini_finish_reasons() {
        let reasons: Vec<_> = ["STOP", "MAX_TOKENS", "SAFETY", "OTHER"]
            .iter()
            .map(|r| {
                json!({
                    "input": r,
                    "ir": format!("{:?}", ir_translate::gemini_finish_reason_to_ir(Some(r)))
                })
            })
            .collect();
        insta::assert_json_snapshot!("gemini_finish_reason_mapping", &reasons);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Codex bridge IR translations (5 tests)
// ═══════════════════════════════════════════════════════════════════════════

mod codex_ir {
    use super::*;
    use abp_codex_sdk::dialect::*;
    use codex_bridge::ir_translate;

    #[test]
    fn codex_minimal_request_to_ir() {
        let req = CodexRequest {
            model: "codex-mini-latest".into(),
            input: vec![CodexInputItem::Message {
                role: "user".into(),
                content: "fn add(a: i32, b: i32) -> i32 { a + b }".into(),
            }],
            tools: vec![],
            temperature: None,
            max_output_tokens: None,
            text: None,
        };
        let ir = ir_translate::codex_request_to_ir(&req);
        insta::assert_json_snapshot!("codex_minimal_request_to_ir", &ir);
    }

    #[test]
    fn codex_request_with_tools_to_ir() {
        let req = CodexRequest {
            model: "codex-mini-latest".into(),
            input: vec![
                CodexInputItem::Message {
                    role: "system".into(),
                    content: "Use tools to answer".into(),
                },
                CodexInputItem::Message {
                    role: "user".into(),
                    content: "What files exist?".into(),
                },
            ],
            tools: vec![CodexTool::Function {
                function: CodexFunctionDef {
                    name: "list_files".into(),
                    description: "List directory".into(),
                    parameters: json!({"type":"object","properties":{"dir":{"type":"string"}}}),
                },
            }],
            temperature: Some(0.3),
            max_output_tokens: Some(512),
            text: None,
        };
        let ir = ir_translate::codex_request_to_ir(&req);
        insta::assert_json_snapshot!("codex_request_with_tools_to_ir", &ir);
    }

    #[test]
    fn codex_response_to_ir() {
        let resp = CodexResponse {
            id: "resp_codex_01".into(),
            model: "codex-mini-latest".into(),
            output: vec![CodexResponseItem::Message {
                role: "assistant".into(),
                content: vec![CodexContentPart::OutputText {
                    text: "Here are the test cases.".into(),
                }],
            }],
            usage: Some(CodexUsage {
                input_tokens: 40,
                output_tokens: 60,
                total_tokens: 100,
            }),
            status: Some("completed".into()),
        };
        let ir = ir_translate::codex_response_to_ir(&resp);
        insta::assert_json_snapshot!("codex_response_to_ir", &ir);
    }

    #[test]
    fn codex_ir_roundtrip_request() {
        let ir = DialectIrRequest::new(vec![DialectIrMessage::text(
            DialectIrRole::User,
            "Generate unit tests",
        )])
        .with_model("codex-mini-latest");
        let codex = ir_translate::ir_to_codex_request(&ir);
        let back = ir_translate::codex_request_to_ir(&codex);
        insta::assert_json_snapshot!("codex_ir_roundtrip_request", &back);
    }

    #[test]
    fn codex_stream_event_to_ir() {
        let event = CodexStreamEvent::OutputItemDelta {
            output_index: 0,
            delta: CodexStreamDelta::OutputTextDelta {
                text: "fn test".into(),
            },
        };
        let ir_events = ir_translate::codex_stream_to_ir(&event);
        insta::assert_json_snapshot!("codex_stream_event_to_ir", &ir_events);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Copilot bridge IR translations (5 tests)
// ═══════════════════════════════════════════════════════════════════════════

mod copilot_ir {
    use super::*;
    use copilot_bridge::copilot_types::*;
    use copilot_bridge::ir_translate;

    #[test]
    fn copilot_minimal_request_to_ir() {
        let req = CopilotChatRequest {
            model: "gpt-4o".into(),
            messages: vec![CopilotMessage {
                role: CopilotMessageRole::User,
                content: Some("Hello".into()),
                tool_calls: None,
                tool_call_id: None,
                name: None,
                copilot_references: vec![],
            }],
            tools: None,
            temperature: None,
            max_tokens: None,
            stream: None,
            top_p: None,
            n: None,
            stop: None,
            tool_choice: None,
            copilot_references: vec![],
            turn_history: vec![],
        };
        let ir = ir_translate::copilot_request_to_ir(&req);
        insta::assert_json_snapshot!("copilot_minimal_request_to_ir", &ir);
    }

    #[test]
    fn copilot_full_request_to_ir() {
        let req = CopilotChatRequest {
            model: "gpt-4o".into(),
            messages: vec![
                CopilotMessage {
                    role: CopilotMessageRole::System,
                    content: Some("You are a coding assistant.".into()),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                    copilot_references: vec![],
                },
                CopilotMessage {
                    role: CopilotMessageRole::User,
                    content: Some("Fix this bug".into()),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                    copilot_references: vec![],
                },
            ],
            tools: Some(vec![CopilotTool {
                tool_type: CopilotToolType::Function,
                function: Some(CopilotFunctionDef {
                    name: "read_file".into(),
                    description: "Read a file".into(),
                    parameters: json!({"type":"object","properties":{"path":{"type":"string"}}}),
                }),
            }]),
            temperature: Some(0.5),
            max_tokens: Some(2048),
            stream: Some(true),
            top_p: Some(0.9),
            n: Some(1),
            stop: None,
            tool_choice: None,
            copilot_references: vec![CopilotReference {
                ref_type: CopilotReferenceType::File,
                id: "src/main.rs".into(),
                data: json!({"content": "fn main() {}"}),
                metadata: None,
            }],
            turn_history: vec![],
        };
        let ir = ir_translate::copilot_request_to_ir(&req);
        insta::assert_json_snapshot!("copilot_full_request_to_ir", &ir);
    }

    #[test]
    fn copilot_response_to_ir() {
        let resp = CopilotChatResponse {
            id: Some("chatcmpl-copilot".into()),
            model: Some("gpt-4o".into()),
            choices: vec![CopilotChatChoice {
                index: 0,
                message: CopilotMessage {
                    role: CopilotMessageRole::Assistant,
                    content: Some("Fixed the bug.".into()),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                    copilot_references: vec![],
                },
                finish_reason: Some("stop".into()),
            }],
            usage: Some(CopilotUsage {
                prompt_tokens: 60,
                completion_tokens: 10,
                total_tokens: 70,
            }),
            copilot_references: vec![],
            copilot_errors: vec![],
            copilot_confirmation: None,
        };
        let ir = ir_translate::copilot_response_to_ir(&resp);
        insta::assert_json_snapshot!("copilot_response_to_ir", &ir);
    }

    #[test]
    fn copilot_ir_roundtrip_request() {
        let ir = IrChatRequest::new(
            "gpt-4o",
            vec![IrMessage::text(IrRole::User, "Explain this error")],
        )
        .with_max_tokens(1024);
        let copilot = ir_translate::ir_to_copilot_request(&ir);
        let back = ir_translate::copilot_request_to_ir(&copilot);
        insta::assert_json_snapshot!("copilot_ir_roundtrip_request", &back);
    }

    #[test]
    fn copilot_stream_event_to_ir() {
        let event = CopilotStreamEvent::ChatCompletionChunk {
            chunk: CopilotStreamChunk {
                id: Some("chunk-01".into()),
                model: Some("gpt-4o".into()),
                choices: vec![CopilotStreamChoice {
                    index: 0,
                    delta: CopilotStreamDelta {
                        role: Some("assistant".into()),
                        content: Some("Hello".into()),
                        tool_calls: None,
                    },
                    finish_reason: None,
                }],
                usage: None,
            },
        };
        let ir_chunks = ir_translate::copilot_stream_to_ir(&event);
        insta::assert_json_snapshot!("copilot_stream_event_to_ir", &ir_chunks);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Kimi bridge IR translations (5 tests)
// ═══════════════════════════════════════════════════════════════════════════

mod kimi_ir {
    use super::*;
    use kimi_bridge::ir_translate;
    use kimi_bridge::kimi_types::*;

    #[test]
    fn kimi_minimal_request_to_ir() {
        let req = KimiRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![Message {
                role: Role::User,
                content: Some("Hello".into()),
                tool_call_id: None,
                tool_calls: None,
            }],
            max_tokens: None,
            temperature: None,
            stream: None,
            tools: None,
            use_search: None,
        };
        let ir = ir_translate::kimi_request_to_ir(&req);
        insta::assert_json_snapshot!("kimi_minimal_request_to_ir", &ir);
    }

    #[test]
    fn kimi_full_request_to_ir() {
        let req = KimiRequest {
            model: "moonshot-v1-128k".into(),
            messages: vec![
                Message {
                    role: Role::System,
                    content: Some("You are a Chinese language expert.".into()),
                    tool_call_id: None,
                    tool_calls: None,
                },
                Message {
                    role: Role::User,
                    content: Some("Translate: hello".into()),
                    tool_call_id: None,
                    tool_calls: None,
                },
            ],
            max_tokens: Some(2048),
            temperature: Some(0.3),
            stream: Some(false),
            tools: Some(vec![ToolDefinition::Function {
                function: FunctionDefinition {
                    name: "dictionary".into(),
                    description: "Look up a word".into(),
                    parameters: json!({"type":"object","properties":{"word":{"type":"string"}}}),
                },
            }]),
            use_search: None,
        };
        let ir = ir_translate::kimi_request_to_ir(&req);
        insta::assert_json_snapshot!("kimi_full_request_to_ir", &ir);
    }

    #[test]
    fn kimi_response_to_ir() {
        let resp = KimiResponse {
            id: "cmpl-kimi-01".into(),
            model: "moonshot-v1-8k".into(),
            choices: vec![Choice {
                index: 0,
                message: ResponseMessage {
                    role: "assistant".into(),
                    content: Some("你好 means hello.".into()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: Some(Usage {
                prompt_tokens: 20,
                completion_tokens: 8,
                total_tokens: 28,
            }),
            refs: None,
        };
        let ir = ir_translate::kimi_response_to_ir(&resp);
        insta::assert_json_snapshot!("kimi_response_to_ir", &ir);
    }

    #[test]
    fn kimi_ir_roundtrip_request() {
        let ir = DialectIrRequest::new(vec![DialectIrMessage::text(
            DialectIrRole::User,
            "Translate: goodbye",
        )])
        .with_model("moonshot-v1-8k");
        let kimi = ir_translate::ir_to_kimi_request(&ir);
        let back = ir_translate::kimi_request_to_ir(&kimi);
        insta::assert_json_snapshot!("kimi_ir_roundtrip_request", &back);
    }

    #[test]
    fn kimi_stream_chunk_to_ir() {
        let chunk = StreamChunk {
            id: "chunk-01".into(),
            object: "chat.completion.chunk".into(),
            created: 1700000000,
            model: "moonshot-v1-8k".into(),
            choices: vec![StreamChoice {
                index: 0,
                delta: StreamDelta {
                    role: Some("assistant".into()),
                    content: Some("你好".into()),
                    tool_calls: None,
                },
                finish_reason: None,
            }],
            usage: None,
            refs: None,
        };
        let ir_events = ir_translate::kimi_stream_to_ir(&chunk);
        insta::assert_json_snapshot!("kimi_stream_chunk_to_ir", &ir_events);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Canonical receipt JSON format (4 tests)
// ═══════════════════════════════════════════════════════════════════════════

mod receipt_canonical {
    use super::*;

    #[test]
    fn receipt_canonical_json() {
        let receipt = sample_receipt();
        insta::assert_yaml_snapshot!("receipt_canonical_json", &receipt);
    }

    #[test]
    fn receipt_minimal() {
        let receipt = Receipt {
            meta: RunMetadata {
                run_id: fixed_uuid(),
                work_order_id: fixed_uuid2(),
                contract_version: "abp/v0.1".into(),
                started_at: fixed_ts(),
                finished_at: fixed_ts(),
                duration_ms: 100,
            },
            backend: BackendIdentity {
                id: "mock".into(),
                backend_version: None,
                adapter_version: None,
            },
            capabilities: BTreeMap::new(),
            mode: ExecutionMode::Passthrough,
            usage_raw: json!({}),
            usage: UsageNormalized {
                input_tokens: None,
                output_tokens: None,
                cache_read_tokens: None,
                cache_write_tokens: None,
                request_units: None,
                estimated_cost_usd: None,
            },
            trace: vec![],
            artifacts: vec![],
            verification: VerificationReport {
                git_diff: None,
                git_status: None,
                harness_ok: false,
            },
            outcome: Outcome::Failed,
            receipt_sha256: None,
        };
        insta::assert_json_snapshot!("receipt_minimal", &receipt);
    }

    #[test]
    fn receipt_with_tool_trace() {
        let mut receipt = sample_receipt();
        receipt.trace = vec![
            AgentEvent {
                ts: fixed_ts(),
                kind: AgentEventKind::RunStarted {
                    message: "starting".into(),
                },
                ext: None,
            },
            AgentEvent {
                ts: fixed_ts(),
                kind: AgentEventKind::ToolCall {
                    tool_name: "read_file".into(),
                    tool_use_id: Some("call_01".into()),
                    parent_tool_use_id: None,
                    input: json!({"path": "src/main.rs"}),
                },
                ext: None,
            },
            AgentEvent {
                ts: fixed_ts(),
                kind: AgentEventKind::ToolResult {
                    tool_name: "read_file".into(),
                    tool_use_id: Some("call_01".into()),
                    output: json!("fn main() {}"),
                    is_error: false,
                },
                ext: None,
            },
            AgentEvent {
                ts: fixed_ts(),
                kind: AgentEventKind::AssistantMessage {
                    text: "Done reviewing.".into(),
                },
                ext: None,
            },
            AgentEvent {
                ts: fixed_ts(),
                kind: AgentEventKind::RunCompleted {
                    message: "finished".into(),
                },
                ext: None,
            },
        ];
        insta::assert_yaml_snapshot!("receipt_with_tool_trace", &receipt);
    }

    #[test]
    fn receipt_partial_outcome() {
        let mut receipt = sample_receipt();
        receipt.outcome = Outcome::Partial;
        receipt.trace.push(AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::Warning {
                message: "Context window limit reached".into(),
            },
            ext: None,
        });
        insta::assert_yaml_snapshot!("receipt_partial_outcome", &receipt);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Protocol envelope JSONL format (5 tests)
// ═══════════════════════════════════════════════════════════════════════════

mod protocol_envelopes {
    use super::*;
    use abp_protocol::Envelope;

    #[test]
    fn envelope_hello_snapshot() {
        let env = Envelope::hello(
            BackendIdentity {
                id: "sidecar:openai".into(),
                backend_version: Some("4.0.0".into()),
                adapter_version: Some("0.1.0".into()),
            },
            sample_capabilities(),
        );
        insta::assert_yaml_snapshot!("envelope_hello", &env);
    }

    #[test]
    fn envelope_run_snapshot() {
        let wo = WorkOrderBuilder::new("Write unit tests for the auth module").build();
        let env = Envelope::Run {
            id: fixed_uuid().to_string(),
            work_order: wo,
        };
        insta::assert_json_snapshot!("envelope_run", &env, {
            ".work_order.id" => "[uuid]",
            ".work_order.created_at" => "[timestamp]"
        });
    }

    #[test]
    fn envelope_event_snapshot() {
        let env = Envelope::Event {
            ref_id: fixed_uuid().to_string(),
            event: AgentEvent {
                ts: fixed_ts(),
                kind: AgentEventKind::AssistantDelta {
                    text: "Here is the code:".into(),
                },
                ext: None,
            },
        };
        insta::assert_json_snapshot!("envelope_event", &env);
    }

    #[test]
    fn envelope_final_snapshot() {
        let env = Envelope::Final {
            ref_id: fixed_uuid().to_string(),
            receipt: sample_receipt(),
        };
        insta::assert_yaml_snapshot!("envelope_final", &env);
    }

    #[test]
    fn envelope_fatal_snapshot() {
        let env = Envelope::Fatal {
            ref_id: Some(fixed_uuid().to_string()),
            error: "Backend process crashed unexpectedly".into(),
            error_code: Some(abp_error::ErrorCode::BackendCrashed),
        };
        insta::assert_json_snapshot!("envelope_fatal", &env);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Capability negotiation reports (4 tests)
// ═══════════════════════════════════════════════════════════════════════════

mod capability_negotiation {
    use super::*;
    use abp_capability::{generate_report, negotiate};

    #[test]
    fn negotiation_all_native() {
        let mut manifest = BTreeMap::new();
        manifest.insert(Capability::Streaming, SupportLevel::Native);
        manifest.insert(Capability::ToolUse, SupportLevel::Native);
        let reqs = CapabilityRequirements {
            required: vec![
                CapabilityRequirement {
                    capability: Capability::Streaming,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::ToolUse,
                    min_support: MinSupport::Native,
                },
            ],
        };
        let result = negotiate(&manifest, &reqs);
        let report = generate_report(&result);
        insta::assert_json_snapshot!("negotiation_all_native_report", &report);
    }

    #[test]
    fn negotiation_with_emulated() {
        let mut manifest = BTreeMap::new();
        manifest.insert(Capability::Streaming, SupportLevel::Native);
        manifest.insert(Capability::ExtendedThinking, SupportLevel::Emulated);
        let reqs = CapabilityRequirements {
            required: vec![
                CapabilityRequirement {
                    capability: Capability::Streaming,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::ExtendedThinking,
                    min_support: MinSupport::Emulated,
                },
            ],
        };
        let result = negotiate(&manifest, &reqs);
        let report = generate_report(&result);
        insta::assert_json_snapshot!("negotiation_with_emulated_report", &report);
    }

    #[test]
    fn negotiation_with_unsupported() {
        let mut manifest = BTreeMap::new();
        manifest.insert(Capability::Streaming, SupportLevel::Native);
        manifest.insert(Capability::ToolUse, SupportLevel::Unsupported);
        let reqs = CapabilityRequirements {
            required: vec![
                CapabilityRequirement {
                    capability: Capability::Streaming,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::ToolUse,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::CodeExecution,
                    min_support: MinSupport::Native,
                },
            ],
        };
        let result = negotiate(&manifest, &reqs);
        let report = generate_report(&result);
        insta::assert_json_snapshot!("negotiation_unsupported_report", &report);
    }

    #[test]
    fn negotiation_empty_requirements() {
        let manifest = sample_capabilities();
        let reqs = CapabilityRequirements { required: vec![] };
        let result = negotiate(&manifest, &reqs);
        let report = generate_report(&result);
        insta::assert_json_snapshot!("negotiation_empty_reqs_report", &report);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Error taxonomy formatted outputs (5 tests)
// ═══════════════════════════════════════════════════════════════════════════

mod error_taxonomy {
    use super::*;
    use abp_error::ErrorCode;
    use abp_error_taxonomy::ErrorClassifier;

    #[test]
    fn classify_backend_timeout() {
        let classifier = ErrorClassifier::new();
        let classification = classifier.classify(&ErrorCode::BackendTimeout);
        insta::assert_json_snapshot!("error_classify_backend_timeout", &classification);
    }

    #[test]
    fn classify_protocol_handshake_failed() {
        let classifier = ErrorClassifier::new();
        let classification = classifier.classify(&ErrorCode::ProtocolHandshakeFailed);
        insta::assert_json_snapshot!("error_classify_protocol_handshake", &classification);
    }

    #[test]
    fn classify_mapping_lossy_conversion() {
        let classifier = ErrorClassifier::new();
        let classification = classifier.classify(&ErrorCode::MappingLossyConversion);
        insta::assert_json_snapshot!("error_classify_mapping_lossy", &classification);
    }

    #[test]
    fn classify_capability_unsupported() {
        let classifier = ErrorClassifier::new();
        let classification = classifier.classify(&ErrorCode::CapabilityUnsupported);
        insta::assert_json_snapshot!("error_classify_capability_unsupported", &classification);
    }

    #[test]
    fn classify_backend_rate_limited() {
        let classifier = ErrorClassifier::new();
        let classification = classifier.classify(&ErrorCode::BackendRateLimited);
        let recovery = classifier.suggest_recovery(&classification);
        insta::assert_json_snapshot!(
            "error_classify_rate_limited_recovery",
            &json!({
                "classification": classification,
                "recovery": recovery,
            })
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. Projection matrix translation results (4 tests)
// ═══════════════════════════════════════════════════════════════════════════

mod projection_matrix {
    use super::*;
    use abp_dialect::Dialect;
    use abp_projection::{DialectPair, ProjectionConfig, ProjectionMatrix, ProjectionMode};

    #[test]
    fn projection_dialect_pair_snapshot() {
        let pair = DialectPair::new(Dialect::OpenAi, Dialect::Claude);
        insta::assert_json_snapshot!("projection_dialect_pair_openai_claude", &pair);
    }

    #[test]
    fn projection_config_default() {
        let config = ProjectionConfig::default();
        insta::assert_json_snapshot!("projection_config_default", &config);
    }

    #[test]
    fn projection_register_and_lookup() {
        let mut matrix = ProjectionMatrix::new();
        matrix.register(Dialect::OpenAi, Dialect::Claude, ProjectionMode::Mapped);
        matrix.register(
            Dialect::Claude,
            Dialect::Claude,
            ProjectionMode::Passthrough,
        );
        matrix.register(Dialect::Gemini, Dialect::OpenAi, ProjectionMode::Mapped);
        let lookup_mapped = matrix.lookup(Dialect::OpenAi, Dialect::Claude);
        let lookup_pass = matrix.lookup(Dialect::Claude, Dialect::Claude);
        let lookup_missing = matrix.lookup(Dialect::Kimi, Dialect::Codex);
        insta::assert_json_snapshot!(
            "projection_lookup_results",
            &json!({
                "openai_to_claude": format!("{:?}", lookup_mapped),
                "claude_to_claude": format!("{:?}", lookup_pass),
                "kimi_to_codex": format!("{:?}", lookup_missing),
            })
        );
    }

    #[test]
    fn projection_all_dialects_pairs() {
        let dialects = [
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
            Dialect::Kimi,
            Dialect::Copilot,
        ];
        let pairs: Vec<_> = dialects
            .iter()
            .map(|d| {
                json!({
                    "dialect": format!("{:?}", d),
                    "self_pair": DialectPair::new(*d, *d),
                })
            })
            .collect();
        insta::assert_json_snapshot!("projection_all_self_pairs", &pairs);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. Streaming event sequences (5 tests)
// ═══════════════════════════════════════════════════════════════════════════

mod streaming_sequences {
    use super::*;

    #[test]
    fn ir_stream_text_sequence() {
        let chunks = vec![
            IrStreamChunk {
                id: Some("stream-001".into()),
                model: Some("gpt-4o".into()),
                index: 0,
                delta_content: vec![],
                delta_tool_calls: vec![],
                role: Some(IrRole::Assistant),
                finish_reason: None,
                usage: None,
                metadata: BTreeMap::new(),
            },
            IrStreamChunk::text_delta("Hello, "),
            IrStreamChunk::text_delta("how can I "),
            IrStreamChunk::text_delta("help you?"),
            IrStreamChunk {
                id: None,
                model: None,
                index: 0,
                delta_content: vec![],
                delta_tool_calls: vec![],
                role: None,
                finish_reason: Some(IrFinishReason::Stop),
                usage: Some(IrUsage::from_counts(10, 8)),
                metadata: BTreeMap::new(),
            },
        ];
        insta::assert_json_snapshot!("ir_stream_text_sequence", &chunks);
    }

    #[test]
    fn ir_stream_tool_call_sequence() {
        let chunks = vec![
            IrStreamChunk {
                id: Some("stream-002".into()),
                model: Some("gpt-4o".into()),
                index: 0,
                delta_content: vec![],
                delta_tool_calls: vec![],
                role: Some(IrRole::Assistant),
                finish_reason: None,
                usage: None,
                metadata: BTreeMap::new(),
            },
            IrStreamChunk {
                id: None,
                model: None,
                index: 0,
                delta_content: vec![],
                delta_tool_calls: vec![IrToolCall {
                    id: "call_weather".into(),
                    name: "get_weather".into(),
                    arguments: json!({"city": "London"}),
                }],
                role: None,
                finish_reason: None,
                usage: None,
                metadata: BTreeMap::new(),
            },
            IrStreamChunk::final_chunk(IrFinishReason::ToolUse),
        ];
        insta::assert_json_snapshot!("ir_stream_tool_call_sequence", &chunks);
    }

    #[test]
    fn dialect_ir_stream_text_sequence() {
        let events = vec![
            IrStreamEvent::stream_start(),
            IrStreamEvent::ContentBlockStart {
                index: 0,
                block: IrContentBlock::Text {
                    text: String::new(),
                },
            },
            IrStreamEvent::text_delta(0, "Hello "),
            IrStreamEvent::text_delta(0, "world"),
            IrStreamEvent::ContentBlockStop { index: 0 },
            IrStreamEvent::Usage {
                usage: DialectIrUsage::from_io(15, 4),
            },
            IrStreamEvent::stream_end(Some(IrStopReason::EndTurn)),
        ];
        insta::assert_json_snapshot!("dialect_ir_stream_text_sequence", &events);
    }

    #[test]
    fn dialect_ir_stream_tool_sequence() {
        let events = vec![
            IrStreamEvent::stream_start(),
            IrStreamEvent::ContentBlockStart {
                index: 0,
                block: IrContentBlock::ToolCall {
                    id: "call_01".into(),
                    name: "read_file".into(),
                    input: json!({}),
                },
            },
            IrStreamEvent::tool_call_delta(0, r#"{"path":""#),
            IrStreamEvent::tool_call_delta(0, r#"src/main.rs"}"#),
            IrStreamEvent::ContentBlockStop { index: 0 },
            IrStreamEvent::stream_end(Some(IrStopReason::ToolUse)),
        ];
        insta::assert_json_snapshot!("dialect_ir_stream_tool_sequence", &events);
    }

    #[test]
    fn dialect_ir_stream_error_event() {
        let events = vec![
            IrStreamEvent::stream_start(),
            IrStreamEvent::text_delta(0, "Partial output..."),
            IrStreamEvent::error("rate_limit", "Rate limit exceeded, retry after 30s"),
        ];
        insta::assert_json_snapshot!("dialect_ir_stream_error_sequence", &events);
    }
}
