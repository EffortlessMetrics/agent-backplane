// SPDX-License-Identifier: MIT OR Apache-2.0
//! Public OpenAI Chat Completions API surface types and conversions to/from ABP core.
//!
//! This module provides first-class Rust types matching the OpenAI Chat Completions
//! API surface, plus `From` trait conversions to map between these types and
//! [`abp_core::WorkOrder`] / [`abp_core::Receipt`].

use abp_core::{AgentEvent, AgentEventKind, Outcome, Receipt, WorkOrder, WorkOrderBuilder};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::response_format::ResponseFormat;

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

/// A Chat Completions API request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatCompletionRequest {
    /// Model identifier (e.g. `gpt-4o`).
    pub model: String,
    /// Conversation messages.
    pub messages: Vec<Message>,
    /// Sampling temperature (0.0–2.0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Maximum tokens to generate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Tool definitions available to the model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    /// Controls which tool the model should call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
    /// Whether to stream the response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    /// Nucleus sampling parameter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    /// Frequency penalty (−2.0 to 2.0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,
    /// Presence penalty (−2.0 to 2.0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,
    /// Up to 4 stop sequences.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    /// Number of completions to generate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<u32>,
    /// Deterministic sampling seed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    /// Response format constraint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,
    /// A unique identifier for the end-user.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
}

/// A message in a Chat Completions conversation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum Message {
    /// System instruction message.
    System {
        /// The system instruction text.
        content: String,
    },
    /// User message.
    User {
        /// The user message text.
        content: String,
    },
    /// Assistant response message.
    Assistant {
        /// Text content (may be absent when tool calls are present).
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        /// Tool calls requested by the assistant.
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_calls: Option<Vec<ToolCall>>,
    },
    /// Tool result message.
    Tool {
        /// The tool call ID this result corresponds to.
        tool_call_id: String,
        /// The tool output content.
        content: String,
    },
}

/// A tool definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Tool {
    /// Tool type — always `"function"`.
    #[serde(rename = "type")]
    pub tool_type: String,
    /// The function definition.
    pub function: FunctionDefinition,
}

/// A function definition inside a [`Tool`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FunctionDefinition {
    /// Function name.
    pub name: String,
    /// Human-readable description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// JSON Schema for the function parameters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<serde_json::Value>,
    /// Whether to enforce strict schema adherence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
}

/// A tool call emitted by the model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCall {
    /// Unique identifier for this tool call.
    pub id: String,
    /// Call type — always `"function"`.
    #[serde(rename = "type")]
    pub call_type: String,
    /// The function invocation details.
    pub function: FunctionCall,
}

/// The function invocation inside a [`ToolCall`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FunctionCall {
    /// Name of the function to invoke.
    pub name: String,
    /// JSON-encoded arguments string.
    pub arguments: String,
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// A Chat Completions API response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatCompletionResponse {
    /// Unique response identifier (e.g. `chatcmpl-...`).
    pub id: String,
    /// Object type — always `"chat.completion"`.
    pub object: String,
    /// Unix timestamp of creation.
    pub created: u64,
    /// Model that generated the completion.
    pub model: String,
    /// Completion choices.
    pub choices: Vec<Choice>,
    /// Token usage statistics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// System fingerprint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_fingerprint: Option<String>,
}

/// A single choice in the response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Choice {
    /// Zero-based index of this choice.
    pub index: u32,
    /// The assistant's response message.
    pub message: AssistantMessage,
    /// Reason the model stopped generating.
    pub finish_reason: FinishReason,
}

/// The assistant message inside a [`Choice`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AssistantMessage {
    /// Role — always `"assistant"`.
    #[serde(default = "default_assistant_role")]
    pub role: String,
    /// Text content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Tool calls requested by the assistant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

fn default_assistant_role() -> String {
    "assistant".into()
}

/// The reason a model stopped generating tokens.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    /// Natural stop or hit a stop sequence.
    Stop,
    /// Token limit reached.
    Length,
    /// Model decided to call tool(s).
    ToolCalls,
    /// Content was omitted due to content filter.
    ContentFilter,
}

/// Token usage statistics.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Usage {
    /// Tokens consumed by the prompt.
    pub prompt_tokens: u64,
    /// Tokens generated in the completion.
    pub completion_tokens: u64,
    /// Total tokens (prompt + completion).
    pub total_tokens: u64,
}

// ---------------------------------------------------------------------------
// Streaming types
// ---------------------------------------------------------------------------

/// A single streaming chunk (SSE `chat.completion.chunk`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StreamChunk {
    /// Unique chunk identifier.
    pub id: String,
    /// Object type — always `"chat.completion.chunk"`.
    pub object: String,
    /// Unix timestamp of creation.
    pub created: u64,
    /// Model that generated the chunk.
    pub model: String,
    /// Streaming choices (typically one).
    pub choices: Vec<StreamChoice>,
    /// Token usage (only in the final chunk when `stream_options.include_usage` is set).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

/// A single choice inside a [`StreamChunk`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StreamChoice {
    /// Zero-based index.
    pub index: u32,
    /// The incremental delta.
    pub delta: Delta,
    /// Finish reason (`None` while streaming).
    pub finish_reason: Option<FinishReason>,
}

/// The delta payload inside a streaming choice.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Delta {
    /// Role (only in the first chunk).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Incremental text content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Incremental tool call fragments.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

// ---------------------------------------------------------------------------
// From<ChatCompletionRequest> for WorkOrder
// ---------------------------------------------------------------------------

impl From<ChatCompletionRequest> for WorkOrder {
    fn from(req: ChatCompletionRequest) -> Self {
        // Extract the last user message as the task.
        let task = req
            .messages
            .iter()
            .rev()
            .find_map(|m| match m {
                Message::User { content } => Some(content.clone()),
                _ => None,
            })
            .unwrap_or_default();

        // Collect system messages as context snippets.
        let snippets: Vec<abp_core::ContextSnippet> = req
            .messages
            .iter()
            .enumerate()
            .filter_map(|(i, m)| match m {
                Message::System { content } => Some(abp_core::ContextSnippet {
                    name: format!("system_{i}"),
                    content: content.clone(),
                }),
                _ => None,
            })
            .collect();

        let mut builder = WorkOrderBuilder::new(task).model(&req.model);

        if !snippets.is_empty() {
            builder = builder.context(abp_core::ContextPacket {
                files: vec![],
                snippets,
            });
        }

        builder.build()
    }
}

// ---------------------------------------------------------------------------
// From<Receipt> for ChatCompletionResponse
// ---------------------------------------------------------------------------

impl From<Receipt> for ChatCompletionResponse {
    fn from(receipt: Receipt) -> Self {
        // Collect assistant text from the trace.
        let mut text_parts = Vec::new();
        let mut tool_calls = Vec::new();
        let mut finish = FinishReason::Stop;

        for event in &receipt.trace {
            match &event.kind {
                AgentEventKind::AssistantMessage { text } => {
                    text_parts.push(text.clone());
                }
                AgentEventKind::ToolCall {
                    tool_name,
                    tool_use_id,
                    input,
                    ..
                } => {
                    finish = FinishReason::ToolCalls;
                    tool_calls.push(ToolCall {
                        id: tool_use_id
                            .clone()
                            .unwrap_or_else(|| format!("call_{}", tool_calls.len())),
                        call_type: "function".into(),
                        function: FunctionCall {
                            name: tool_name.clone(),
                            arguments: serde_json::to_string(input).unwrap_or_default(),
                        },
                    });
                }
                AgentEventKind::Error { .. } => {
                    finish = FinishReason::Stop;
                }
                _ => {}
            }
        }

        if receipt.outcome == Outcome::Failed {
            finish = FinishReason::Stop;
        }

        let content = if text_parts.is_empty() {
            None
        } else {
            Some(text_parts.join(""))
        };
        let tool_calls_opt = if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        };

        let usage = {
            let u = &receipt.usage;
            if u.input_tokens.is_some() || u.output_tokens.is_some() {
                let input = u.input_tokens.unwrap_or(0);
                let output = u.output_tokens.unwrap_or(0);
                Some(Usage {
                    prompt_tokens: input,
                    completion_tokens: output,
                    total_tokens: input + output,
                })
            } else {
                None
            }
        };

        let created = receipt.meta.finished_at.timestamp() as u64;

        ChatCompletionResponse {
            id: format!("chatcmpl-{}", receipt.meta.run_id),
            object: "chat.completion".into(),
            created,
            model: receipt.backend.id.clone(),
            choices: vec![Choice {
                index: 0,
                message: AssistantMessage {
                    role: "assistant".into(),
                    content,
                    tool_calls: tool_calls_opt,
                },
                finish_reason: finish,
            }],
            usage,
            system_fingerprint: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Map OpenAI ToolCall → ABP AgentEvent
// ---------------------------------------------------------------------------

/// Convert a slice of [`ToolCall`]s into ABP [`AgentEvent`]s.
pub fn tool_calls_to_events(tool_calls: &[ToolCall]) -> Vec<AgentEvent> {
    let now = Utc::now();
    tool_calls
        .iter()
        .map(|tc| {
            let input = serde_json::from_str(&tc.function.arguments)
                .unwrap_or(serde_json::Value::String(tc.function.arguments.clone()));
            AgentEvent {
                ts: now,
                kind: AgentEventKind::ToolCall {
                    tool_name: tc.function.name.clone(),
                    tool_use_id: Some(tc.id.clone()),
                    parent_tool_use_id: None,
                    input,
                },
                ext: None,
            }
        })
        .collect()
}

/// Convert an ABP [`AgentEvent`] with [`AgentEventKind::ToolCall`] back to a [`ToolCall`].
///
/// Returns `None` if the event is not a tool call.
pub fn event_to_tool_call(event: &AgentEvent) -> Option<ToolCall> {
    match &event.kind {
        AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            input,
            ..
        } => Some(ToolCall {
            id: tool_use_id.clone().unwrap_or_else(|| "call_0".to_string()),
            call_type: "function".into(),
            function: FunctionCall {
                name: tool_name.clone(),
                arguments: serde_json::to_string(input).unwrap_or_default(),
            },
        }),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::{
        BackendIdentity, CapabilityManifest, ExecutionMode, RunMetadata, UsageNormalized,
        VerificationReport,
    };
    use serde_json::json;
    use uuid::Uuid;

    // ── Serde roundtrips ────────────────────────────────────────────────

    #[test]
    fn message_system_serde_roundtrip() {
        let msg = Message::System {
            content: "You are helpful.".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""role":"system""#));
        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, msg);
    }

    #[test]
    fn message_user_serde_roundtrip() {
        let msg = Message::User {
            content: "Hello!".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""role":"user""#));
        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, msg);
    }

    #[test]
    fn message_assistant_text_serde_roundtrip() {
        let msg = Message::Assistant {
            content: Some("Sure!".into()),
            tool_calls: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""role":"assistant""#));
        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, msg);
    }

    #[test]
    fn message_assistant_with_tool_calls_serde_roundtrip() {
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
    fn message_tool_serde_roundtrip() {
        let msg = Message::Tool {
            tool_call_id: "call_1".into(),
            content: "file contents".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""role":"tool""#));
        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, msg);
    }

    #[test]
    fn tool_definition_serde_roundtrip() {
        let tool = Tool {
            tool_type: "function".into(),
            function: FunctionDefinition {
                name: "get_weather".into(),
                description: Some("Get current weather".into()),
                parameters: Some(json!({
                    "type": "object",
                    "properties": {
                        "location": {"type": "string"}
                    },
                    "required": ["location"]
                })),
                strict: Some(true),
            },
        };
        let json = serde_json::to_string(&tool).unwrap();
        assert!(json.contains(r#""type":"function""#));
        let parsed: Tool = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, tool);
    }

    #[test]
    fn function_definition_minimal_serde_roundtrip() {
        let fd = FunctionDefinition {
            name: "noop".into(),
            description: None,
            parameters: None,
            strict: None,
        };
        let json = serde_json::to_string(&fd).unwrap();
        assert!(!json.contains("description"));
        assert!(!json.contains("parameters"));
        assert!(!json.contains("strict"));
        let parsed: FunctionDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, fd);
    }

    #[test]
    fn finish_reason_serde_roundtrip() {
        for (reason, expected_str) in [
            (FinishReason::Stop, "\"stop\""),
            (FinishReason::Length, "\"length\""),
            (FinishReason::ToolCalls, "\"tool_calls\""),
            (FinishReason::ContentFilter, "\"content_filter\""),
        ] {
            let json = serde_json::to_string(&reason).unwrap();
            assert_eq!(json, expected_str);
            let parsed: FinishReason = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, reason);
        }
    }

    #[test]
    fn usage_serde_roundtrip() {
        let usage = Usage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
        };
        let json = serde_json::to_string(&usage).unwrap();
        let parsed: Usage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, usage);
    }

    #[test]
    fn chat_completion_request_full_serde_roundtrip() {
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![
                Message::System {
                    content: "Be helpful.".into(),
                },
                Message::User {
                    content: "Hello".into(),
                },
            ],
            temperature: Some(0.7),
            max_tokens: Some(4096),
            tools: Some(vec![Tool {
                tool_type: "function".into(),
                function: FunctionDefinition {
                    name: "bash".into(),
                    description: Some("Run command".into()),
                    parameters: Some(json!({"type": "object"})),
                    strict: None,
                },
            }]),
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
        let json = serde_json::to_string(&req).unwrap();
        let parsed: ChatCompletionRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, req);
    }

    #[test]
    fn chat_completion_request_omits_none_fields() {
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
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.contains("temperature"));
        assert!(!json.contains("max_tokens"));
        assert!(!json.contains("tools"));
        assert!(!json.contains("stream"));
        assert!(!json.contains("top_p"));
        assert!(!json.contains("stop"));
        assert!(!json.contains("user"));
    }

    #[test]
    fn chat_completion_response_serde_roundtrip() {
        let resp = ChatCompletionResponse {
            id: "chatcmpl-abc".into(),
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
            }],
            usage: Some(Usage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            }),
            system_fingerprint: Some("fp_abc123".into()),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: ChatCompletionResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, resp);
    }

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
                    content: Some("Hi".into()),
                    tool_calls: None,
                },
                finish_reason: None,
            }],
            usage: None,
        };
        let json = serde_json::to_string(&chunk).unwrap();
        let parsed: StreamChunk = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, chunk);
    }

    #[test]
    fn stream_chunk_with_finish_reason_roundtrip() {
        let chunk = StreamChunk {
            id: "chatcmpl-stream-2".into(),
            object: "chat.completion.chunk".into(),
            created: 1700000001,
            model: "gpt-4o".into(),
            choices: vec![StreamChoice {
                index: 0,
                delta: Delta::default(),
                finish_reason: Some(FinishReason::Stop),
            }],
            usage: Some(Usage {
                prompt_tokens: 20,
                completion_tokens: 10,
                total_tokens: 30,
            }),
        };
        let json = serde_json::to_string(&chunk).unwrap();
        let parsed: StreamChunk = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, chunk);
    }

    // ── From<ChatCompletionRequest> for WorkOrder ───────────────────────

    fn make_request(messages: Vec<Message>) -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages,
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
        }
    }

    #[test]
    fn request_to_work_order_uses_last_user_message_as_task() {
        let req = make_request(vec![
            Message::User {
                content: "First".into(),
            },
            Message::User {
                content: "Second".into(),
            },
        ]);
        let wo: WorkOrder = req.into();
        assert_eq!(wo.task, "Second");
    }

    #[test]
    fn request_to_work_order_preserves_model() {
        let mut req = make_request(vec![Message::User {
            content: "Hello".into(),
        }]);
        req.model = "gpt-4-turbo".into();
        let wo: WorkOrder = req.into();
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4-turbo"));
    }

    #[test]
    fn request_to_work_order_maps_system_to_snippets() {
        let req = make_request(vec![
            Message::System {
                content: "Be concise.".into(),
            },
            Message::User {
                content: "Hi".into(),
            },
        ]);
        let wo: WorkOrder = req.into();
        assert_eq!(wo.context.snippets.len(), 1);
        assert_eq!(wo.context.snippets[0].content, "Be concise.");
    }

    #[test]
    fn request_to_work_order_empty_messages_yields_empty_task() {
        let req = make_request(vec![]);
        let wo: WorkOrder = req.into();
        assert_eq!(wo.task, "");
    }

    // ── From<Receipt> for ChatCompletionResponse ────────────────────────

    fn make_receipt(trace: Vec<AgentEvent>, usage: UsageNormalized) -> Receipt {
        let now = Utc::now();
        let run_id = Uuid::new_v4();
        Receipt {
            meta: RunMetadata {
                run_id,
                work_order_id: Uuid::new_v4(),
                contract_version: "abp/v0.1".into(),
                started_at: now,
                finished_at: now,
                duration_ms: 100,
            },
            backend: BackendIdentity {
                id: "openai/gpt-4o".into(),
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
            outcome: Outcome::Complete,
            receipt_sha256: None,
        }
    }

    #[test]
    fn receipt_to_response_maps_assistant_text() {
        let trace = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Hello there!".into(),
            },
            ext: None,
        }];
        let receipt = make_receipt(trace, UsageNormalized::default());
        let resp: ChatCompletionResponse = receipt.into();

        assert_eq!(resp.object, "chat.completion");
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Hello there!")
        );
        assert_eq!(resp.choices[0].finish_reason, FinishReason::Stop);
    }

    #[test]
    fn receipt_to_response_maps_tool_calls() {
        let trace = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("call_abc".into()),
                parent_tool_use_id: None,
                input: json!({"path": "main.rs"}),
            },
            ext: None,
        }];
        let receipt = make_receipt(trace, UsageNormalized::default());
        let resp: ChatCompletionResponse = receipt.into();

        assert_eq!(resp.choices[0].finish_reason, FinishReason::ToolCalls);
        let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tcs.len(), 1);
        assert_eq!(tcs[0].id, "call_abc");
        assert_eq!(tcs[0].function.name, "read_file");
    }

    #[test]
    fn receipt_to_response_maps_usage() {
        let usage = UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            ..UsageNormalized::default()
        };
        let receipt = make_receipt(vec![], usage);
        let resp: ChatCompletionResponse = receipt.into();

        let u = resp.usage.unwrap();
        assert_eq!(u.prompt_tokens, 100);
        assert_eq!(u.completion_tokens, 50);
        assert_eq!(u.total_tokens, 150);
    }

    #[test]
    fn receipt_to_response_no_usage_when_both_none() {
        let receipt = make_receipt(vec![], UsageNormalized::default());
        let resp: ChatCompletionResponse = receipt.into();
        assert!(resp.usage.is_none());
    }

    #[test]
    fn receipt_to_response_concatenates_multiple_assistant_messages() {
        let trace = vec![
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: "Part 1. ".into(),
                },
                ext: None,
            },
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: "Part 2.".into(),
                },
                ext: None,
            },
        ];
        let receipt = make_receipt(trace, UsageNormalized::default());
        let resp: ChatCompletionResponse = receipt.into();
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Part 1. Part 2.")
        );
    }

    #[test]
    fn receipt_to_response_mixed_text_and_tool_calls() {
        let trace = vec![
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: "Let me check.".into(),
                },
                ext: None,
            },
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::ToolCall {
                    tool_name: "bash".into(),
                    tool_use_id: Some("call_1".into()),
                    parent_tool_use_id: None,
                    input: json!({"cmd": "ls"}),
                },
                ext: None,
            },
        ];
        let receipt = make_receipt(trace, UsageNormalized::default());
        let resp: ChatCompletionResponse = receipt.into();

        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Let me check.")
        );
        assert_eq!(
            resp.choices[0].message.tool_calls.as_ref().unwrap().len(),
            1
        );
        assert_eq!(resp.choices[0].finish_reason, FinishReason::ToolCalls);
    }

    #[test]
    fn receipt_to_response_failed_outcome() {
        let mut receipt = make_receipt(vec![], UsageNormalized::default());
        receipt.outcome = Outcome::Failed;
        let resp: ChatCompletionResponse = receipt.into();
        assert_eq!(resp.choices[0].finish_reason, FinishReason::Stop);
    }

    #[test]
    fn receipt_to_response_id_contains_run_id() {
        let receipt = make_receipt(vec![], UsageNormalized::default());
        let run_id = receipt.meta.run_id;
        let resp: ChatCompletionResponse = receipt.into();
        assert!(resp.id.contains(&run_id.to_string()));
    }

    // ── ToolCall mapping helpers ────────────────────────────────────────

    #[test]
    fn tool_calls_to_events_maps_correctly() {
        let tcs = vec![
            ToolCall {
                id: "call_1".into(),
                call_type: "function".into(),
                function: FunctionCall {
                    name: "read_file".into(),
                    arguments: r#"{"path":"a.rs"}"#.into(),
                },
            },
            ToolCall {
                id: "call_2".into(),
                call_type: "function".into(),
                function: FunctionCall {
                    name: "write_file".into(),
                    arguments: r#"{"path":"b.rs","content":"hi"}"#.into(),
                },
            },
        ];
        let events = tool_calls_to_events(&tcs);
        assert_eq!(events.len(), 2);

        match &events[0].kind {
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                input,
                ..
            } => {
                assert_eq!(tool_name, "read_file");
                assert_eq!(tool_use_id.as_deref(), Some("call_1"));
                assert_eq!(input, &json!({"path": "a.rs"}));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn tool_calls_to_events_handles_malformed_arguments() {
        let tcs = vec![ToolCall {
            id: "call_bad".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "foo".into(),
                arguments: "not-json".into(),
            },
        }];
        let events = tool_calls_to_events(&tcs);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::ToolCall { input, .. } => {
                assert_eq!(input, &serde_json::Value::String("not-json".into()));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn event_to_tool_call_roundtrip() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "bash".into(),
                tool_use_id: Some("call_x".into()),
                parent_tool_use_id: None,
                input: json!({"cmd": "ls"}),
            },
            ext: None,
        };
        let tc = event_to_tool_call(&event).unwrap();
        assert_eq!(tc.id, "call_x");
        assert_eq!(tc.function.name, "bash");
        assert_eq!(tc.function.arguments, r#"{"cmd":"ls"}"#);
    }

    #[test]
    fn event_to_tool_call_returns_none_for_non_tool_call() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "hi".into() },
            ext: None,
        };
        assert!(event_to_tool_call(&event).is_none());
    }

    #[test]
    fn event_to_tool_call_without_id_uses_default() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "test".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: json!({}),
            },
            ext: None,
        };
        let tc = event_to_tool_call(&event).unwrap();
        assert_eq!(tc.id, "call_0");
    }

    // ── Response format in request ──────────────────────────────────────

    #[test]
    fn request_with_response_format_roundtrip() {
        let mut req = make_request(vec![Message::User {
            content: "test".into(),
        }]);
        req.response_format = Some(ResponseFormat::json_object());
        let json = serde_json::to_string(&req).unwrap();
        let parsed: ChatCompletionRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.response_format, req.response_format);
    }

    #[test]
    fn request_with_all_optional_fields_roundtrip() {
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![Message::User {
                content: "test".into(),
            }],
            temperature: Some(0.5),
            max_tokens: Some(1024),
            tools: None,
            tool_choice: Some(json!("auto")),
            stream: Some(true),
            top_p: Some(0.9),
            frequency_penalty: Some(0.5),
            presence_penalty: Some(0.3),
            stop: Some(vec!["END".into()]),
            n: Some(2),
            seed: Some(42),
            response_format: Some(ResponseFormat::json_object()),
            user: Some("user-123".into()),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: ChatCompletionRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, req);
    }

    // ── Choice & AssistantMessage ───────────────────────────────────────

    #[test]
    fn choice_serde_roundtrip() {
        let choice = Choice {
            index: 0,
            message: AssistantMessage {
                role: "assistant".into(),
                content: Some("ok".into()),
                tool_calls: None,
            },
            finish_reason: FinishReason::Stop,
        };
        let json = serde_json::to_string(&choice).unwrap();
        let parsed: Choice = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, choice);
    }

    #[test]
    fn delta_default_is_all_none() {
        let d = Delta::default();
        assert!(d.role.is_none());
        assert!(d.content.is_none());
        assert!(d.tool_calls.is_none());
    }
}
