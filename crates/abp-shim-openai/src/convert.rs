// SPDX-License-Identifier: MIT OR Apache-2.0
//! Conversion layer between OpenAI Chat Completions types and ABP core types.
//!
//! This module provides the three main conversion functions that form the core
//! of the drop-in OpenAI API replacement:
//!
//! - [`to_work_order`](crate::convert::to_work_order) — OpenAI request → ABP `WorkOrder`
//! - [`from_receipt`](crate::convert::from_receipt) — ABP `Receipt` + `WorkOrder` → OpenAI response
//! - [`from_agent_event`](crate::convert::from_agent_event) — ABP streaming event → OpenAI SSE chunk

use std::collections::BTreeMap;

use abp_core::{
    AgentEvent, AgentEventKind, Receipt, RuntimeConfig, UsageNormalized, WorkOrder,
    WorkOrderBuilder,
};
use abp_sdk_types::Dialect;
use chrono::Utc;

use crate::types::{
    ChatCompletionRequest, ChatCompletionResponse, ChatMessage, Choice, ChoiceMessage,
    FunctionCall, MessageContent, StreamChoice, StreamChunk, StreamDelta, StreamFunctionCall,
    StreamToolCall, Tool, ToolCall, Usage,
};

// ── Primary conversions ─────────────────────────────────────────────────

/// Convert an OpenAI [`ChatCompletionRequest`] into an ABP [`WorkOrder`].
///
/// Maps:
/// - `messages` → `work_order.task` (extracted from last user message)
/// - `model` → `work_order.config.model`
/// - `temperature`, `top_p`, `max_tokens`, `stream` → `work_order.config.vendor`
/// - `tools` → `work_order.config.vendor["tools"]`
/// - Sets `dialect = Dialect::OpenAi` in vendor config
pub fn to_work_order(req: &ChatCompletionRequest) -> WorkOrder {
    let task = extract_task(req);
    let mut builder = WorkOrderBuilder::new(task).model(req.model.clone());

    let mut vendor = BTreeMap::new();

    // Store dialect marker
    vendor.insert(
        "dialect".to_string(),
        serde_json::to_value(Dialect::OpenAi).unwrap_or_default(),
    );

    if let Some(temp) = req.temperature {
        vendor.insert("temperature".to_string(), serde_json::Value::from(temp));
    }
    if let Some(top_p) = req.top_p {
        vendor.insert("top_p".to_string(), serde_json::Value::from(top_p));
    }
    if let Some(max) = req.max_tokens {
        vendor.insert("max_tokens".to_string(), serde_json::Value::from(max));
    }
    if let Some(stream) = req.stream {
        vendor.insert("stream".to_string(), serde_json::Value::from(stream));
    }
    if let Some(tools) = &req.tools {
        if let Ok(v) = serde_json::to_value(tools) {
            vendor.insert("tools".to_string(), v);
        }
    }
    if let Some(tc) = &req.tool_choice {
        if let Ok(v) = serde_json::to_value(tc) {
            vendor.insert("tool_choice".to_string(), v);
        }
    }

    let config = RuntimeConfig {
        model: Some(req.model.clone()),
        vendor,
        ..Default::default()
    };
    builder = builder.config(config);

    builder.build()
}

/// Convert an ABP [`Receipt`] back into an OpenAI [`ChatCompletionResponse`].
///
/// Walks the receipt trace to reconstruct the assistant message content
/// and any tool calls. Extracts usage from `receipt.usage`.
pub fn from_receipt(receipt: &Receipt, wo: &WorkOrder) -> ChatCompletionResponse {
    let model = wo.config.model.as_deref().unwrap_or("gpt-4o").to_string();

    let mut content: Option<String> = None;
    let mut tool_calls: Vec<ToolCall> = Vec::new();
    let mut finish_reason = "stop".to_string();

    for event in &receipt.trace {
        match &event.kind {
            AgentEventKind::AssistantMessage { text } => {
                content = Some(text.clone());
            }
            AgentEventKind::AssistantDelta { text } => {
                let c = content.get_or_insert_with(String::new);
                c.push_str(text);
            }
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                input,
                ..
            } => {
                tool_calls.push(ToolCall {
                    id: tool_use_id
                        .clone()
                        .unwrap_or_else(|| format!("call_{}", uuid::Uuid::new_v4())),
                    call_type: "function".into(),
                    function: FunctionCall {
                        name: tool_name.clone(),
                        arguments: serde_json::to_string(input).unwrap_or_default(),
                    },
                });
                finish_reason = "tool_calls".to_string();
            }
            AgentEventKind::Error { message, .. } => {
                content = Some(format!("Error: {message}"));
                finish_reason = "stop".to_string();
            }
            _ => {}
        }
    }

    let message = ChoiceMessage {
        role: "assistant".into(),
        content,
        tool_calls: if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        },
    };

    let usage = usage_from_normalized(&receipt.usage);

    ChatCompletionResponse {
        id: format!("chatcmpl-{}", receipt.meta.run_id),
        object: "chat.completion".into(),
        created: receipt.meta.started_at.timestamp() as u64,
        model,
        choices: vec![Choice {
            index: 0,
            message,
            finish_reason: Some(finish_reason),
        }],
        usage: Some(usage),
    }
}

/// Convert a single ABP [`AgentEvent`] into an OpenAI [`StreamChunk`].
///
/// Returns `None` for event kinds that have no OpenAI streaming equivalent
/// (e.g. `FileChanged`, `CommandExecuted`, `Warning`).
///
/// Mapping:
/// - `AssistantDelta` → `delta.content`
/// - `AssistantMessage` → `delta.content` (with role)
/// - `ToolCall` → `delta.tool_calls`
/// - `RunCompleted` → `finish_reason: "stop"`
/// - `Error` → `finish_reason: "stop"` with error in content
pub fn from_agent_event(event: &AgentEvent, model: &str, chunk_id: &str) -> Option<StreamChunk> {
    let created = event.ts.timestamp() as u64;

    match &event.kind {
        AgentEventKind::AssistantDelta { text } => Some(make_stream_chunk(
            chunk_id,
            created,
            model,
            StreamDelta {
                role: None,
                content: Some(text.clone()),
                tool_calls: None,
            },
            None,
        )),
        AgentEventKind::AssistantMessage { text } => Some(make_stream_chunk(
            chunk_id,
            created,
            model,
            StreamDelta {
                role: Some("assistant".into()),
                content: Some(text.clone()),
                tool_calls: None,
            },
            None,
        )),
        AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            input,
            ..
        } => Some(make_stream_chunk(
            chunk_id,
            created,
            model,
            StreamDelta {
                role: None,
                content: None,
                tool_calls: Some(vec![StreamToolCall {
                    index: 0,
                    id: tool_use_id.clone(),
                    call_type: Some("function".into()),
                    function: Some(StreamFunctionCall {
                        name: Some(tool_name.clone()),
                        arguments: Some(serde_json::to_string(input).unwrap_or_default()),
                    }),
                }]),
            },
            None,
        )),
        AgentEventKind::RunCompleted { .. } => Some(make_stream_chunk(
            chunk_id,
            created,
            model,
            StreamDelta::default(),
            Some("stop".into()),
        )),
        AgentEventKind::Error { message, .. } => Some(make_stream_chunk(
            chunk_id,
            created,
            model,
            StreamDelta {
                role: None,
                content: Some(format!("Error: {message}")),
                tool_calls: None,
            },
            Some("stop".into()),
        )),
        // Events with no streaming equivalent
        _ => None,
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────

/// Extract a task description from the request messages.
///
/// Uses the last user message text content, falling back to `"chat completion"`.
pub fn extract_task(req: &ChatCompletionRequest) -> String {
    req.messages
        .iter()
        .rev()
        .find_map(|m| match m {
            ChatMessage::User { content } => Some(message_content_to_string(content)),
            _ => None,
        })
        .unwrap_or_else(|| "chat completion".into())
}

/// Convert [`MessageContent`] to a plain string.
pub fn message_content_to_string(content: &MessageContent) -> String {
    match content {
        MessageContent::Text(s) => s.clone(),
        MessageContent::Parts(parts) => parts
            .iter()
            .filter_map(|p| match p {
                crate::types::ContentPart::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(""),
    }
}

/// Map an OpenAI role string to the corresponding [`ChatMessage`] variant name.
pub fn role_to_str(msg: &ChatMessage) -> &'static str {
    match msg {
        ChatMessage::System { .. } => "system",
        ChatMessage::User { .. } => "user",
        ChatMessage::Assistant { .. } => "assistant",
        ChatMessage::Tool { .. } => "tool",
    }
}

/// Convert OpenAI [`Tool`] definitions into a JSON array suitable for vendor config.
pub fn tools_to_json(tools: &[Tool]) -> serde_json::Value {
    serde_json::to_value(tools).unwrap_or(serde_json::Value::Array(vec![]))
}

/// Convert ABP normalized usage into OpenAI [`Usage`].
pub fn usage_from_normalized(usage: &UsageNormalized) -> Usage {
    let prompt = usage.input_tokens.unwrap_or(0);
    let completion = usage.output_tokens.unwrap_or(0);
    Usage {
        prompt_tokens: prompt,
        completion_tokens: completion,
        total_tokens: prompt + completion,
    }
}

/// Build a [`StreamChunk`] with a single choice.
fn make_stream_chunk(
    id: &str,
    created: u64,
    model: &str,
    delta: StreamDelta,
    finish_reason: Option<String>,
) -> StreamChunk {
    StreamChunk {
        id: id.to_string(),
        object: "chat.completion.chunk".into(),
        created,
        model: model.to_string(),
        choices: vec![StreamChoice {
            index: 0,
            delta,
            finish_reason,
        }],
    }
}

/// Create a terminal "stop" [`StreamChunk`] to signal end-of-stream.
pub fn make_stop_chunk(model: &str, chunk_id: &str) -> StreamChunk {
    make_stream_chunk(
        chunk_id,
        Utc::now().timestamp() as u64,
        model,
        StreamDelta::default(),
        Some("stop".into()),
    )
}

/// Count the number of messages in each role for diagnostics.
pub fn count_roles(req: &ChatCompletionRequest) -> BTreeMap<&'static str, usize> {
    let mut counts = BTreeMap::new();
    for msg in &req.messages {
        let role = role_to_str(msg);
        *counts.entry(role).or_insert(0) += 1;
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;
    use abp_core::{
        AgentEvent, AgentEventKind, Outcome, ReceiptBuilder, UsageNormalized, WorkOrderBuilder,
    };
    use chrono::Utc;
    use serde_json::json;

    // ── Helper: build a minimal request ─────────────────────────────────

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

    fn request_with_system() -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: "gpt-4o".into(),
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
        }
    }

    fn request_with_tools() -> ChatCompletionRequest {
        ChatCompletionRequest {
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
                    description: "Get weather for a location".into(),
                    parameters: json!({"type": "object", "properties": {"location": {"type": "string"}}}),
                },
            }]),
            tool_choice: None,
        }
    }

    fn mock_receipt(events: Vec<AgentEvent>) -> abp_core::Receipt {
        mock_receipt_with_usage(events, UsageNormalized::default())
    }

    fn mock_receipt_with_usage(
        events: Vec<AgentEvent>,
        usage: UsageNormalized,
    ) -> abp_core::Receipt {
        let mut builder = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .usage(usage);
        for e in events {
            builder = builder.add_trace_event(e);
        }
        builder.build()
    }

    fn make_event(kind: AgentEventKind) -> AgentEvent {
        AgentEvent {
            ts: Utc::now(),
            kind,
            ext: None,
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // to_work_order tests
    // ═══════════════════════════════════════════════════════════════════

    // 1
    #[test]
    fn to_work_order_sets_model() {
        let req = minimal_request();
        let wo = to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
    }

    // 2
    #[test]
    fn to_work_order_extracts_task_from_user_message() {
        let req = minimal_request();
        let wo = to_work_order(&req);
        assert_eq!(wo.task, "Hello");
    }

    // 3
    #[test]
    fn to_work_order_maps_temperature() {
        let req = request_with_system();
        let wo = to_work_order(&req);
        assert_eq!(
            wo.config.vendor.get("temperature"),
            Some(&serde_json::Value::from(0.7))
        );
    }

    // 4
    #[test]
    fn to_work_order_maps_top_p() {
        let req = request_with_system();
        let wo = to_work_order(&req);
        assert_eq!(
            wo.config.vendor.get("top_p"),
            Some(&serde_json::Value::from(0.9))
        );
    }

    // 5
    #[test]
    fn to_work_order_maps_max_tokens() {
        let req = request_with_system();
        let wo = to_work_order(&req);
        assert_eq!(
            wo.config.vendor.get("max_tokens"),
            Some(&serde_json::Value::from(1024))
        );
    }

    // 6
    #[test]
    fn to_work_order_maps_stream() {
        let req = request_with_system();
        let wo = to_work_order(&req);
        assert_eq!(
            wo.config.vendor.get("stream"),
            Some(&serde_json::Value::from(true))
        );
    }

    // 7
    #[test]
    fn to_work_order_sets_dialect() {
        let req = minimal_request();
        let wo = to_work_order(&req);
        let dialect: Dialect = serde_json::from_value(wo.config.vendor["dialect"].clone()).unwrap();
        assert_eq!(dialect, Dialect::OpenAi);
    }

    // 8
    #[test]
    fn to_work_order_maps_tools() {
        let req = request_with_tools();
        let wo = to_work_order(&req);
        assert!(wo.config.vendor.contains_key("tools"));
        let tools_val = &wo.config.vendor["tools"];
        assert!(tools_val.is_array());
    }

    // 9
    #[test]
    fn to_work_order_no_tools_omits_key() {
        let req = minimal_request();
        let wo = to_work_order(&req);
        assert!(!wo.config.vendor.contains_key("tools"));
    }

    // 10
    #[test]
    fn to_work_order_no_optional_params() {
        let req = minimal_request();
        let wo = to_work_order(&req);
        assert!(!wo.config.vendor.contains_key("temperature"));
        assert!(!wo.config.vendor.contains_key("top_p"));
        assert!(!wo.config.vendor.contains_key("max_tokens"));
        assert!(!wo.config.vendor.contains_key("stream"));
    }

    // 11
    #[test]
    fn to_work_order_empty_messages_fallback_task() {
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
        let wo = to_work_order(&req);
        assert_eq!(wo.task, "chat completion");
    }

    // 12
    #[test]
    fn to_work_order_system_only_fallback_task() {
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![ChatMessage::System {
                content: "Be helpful".into(),
            }],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
        };
        let wo = to_work_order(&req);
        assert_eq!(wo.task, "chat completion");
    }

    // 13
    #[test]
    fn to_work_order_multi_user_uses_last() {
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![
                ChatMessage::User {
                    content: MessageContent::Text("First".into()),
                },
                ChatMessage::User {
                    content: MessageContent::Text("Second".into()),
                },
            ],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
        };
        let wo = to_work_order(&req);
        assert_eq!(wo.task, "Second");
    }

    // 14
    #[test]
    fn to_work_order_tool_choice_mapped() {
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
            tool_choice: Some(ToolChoice::Mode(ToolChoiceMode::Auto)),
        };
        let wo = to_work_order(&req);
        assert!(wo.config.vendor.contains_key("tool_choice"));
    }

    // ═══════════════════════════════════════════════════════════════════
    // from_receipt tests
    // ═══════════════════════════════════════════════════════════════════

    // 15
    #[test]
    fn from_receipt_assistant_message() {
        let events = vec![make_event(AgentEventKind::AssistantMessage {
            text: "Hello!".into(),
        })];
        let receipt = mock_receipt(events);
        let wo = WorkOrderBuilder::new("test").model("gpt-4o").build();
        let resp = from_receipt(&receipt, &wo);

        assert_eq!(resp.object, "chat.completion");
        assert_eq!(resp.model, "gpt-4o");
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hello!"));
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    // 16
    #[test]
    fn from_receipt_tool_call() {
        let events = vec![make_event(AgentEventKind::ToolCall {
            tool_name: "get_weather".into(),
            tool_use_id: Some("call_abc".into()),
            parent_tool_use_id: None,
            input: json!({"location": "NYC"}),
        })];
        let receipt = mock_receipt(events);
        let wo = WorkOrderBuilder::new("test").model("gpt-4o").build();
        let resp = from_receipt(&receipt, &wo);

        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
        let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tcs.len(), 1);
        assert_eq!(tcs[0].id, "call_abc");
        assert_eq!(tcs[0].function.name, "get_weather");
        assert!(tcs[0].function.arguments.contains("NYC"));
    }

    // 17
    #[test]
    fn from_receipt_error_event() {
        let events = vec![make_event(AgentEventKind::Error {
            message: "rate limit".into(),
            error_code: None,
        })];
        let receipt = mock_receipt(events);
        let wo = WorkOrderBuilder::new("test").model("gpt-4o").build();
        let resp = from_receipt(&receipt, &wo);

        let content = resp.choices[0].message.content.as_deref().unwrap();
        assert!(content.contains("rate limit"));
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    // 18
    #[test]
    fn from_receipt_assembles_deltas() {
        let events = vec![
            make_event(AgentEventKind::AssistantDelta { text: "Hel".into() }),
            make_event(AgentEventKind::AssistantDelta { text: "lo!".into() }),
        ];
        let receipt = mock_receipt(events);
        let wo = WorkOrderBuilder::new("test").model("gpt-4o").build();
        let resp = from_receipt(&receipt, &wo);

        assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hello!"));
    }

    // 19
    #[test]
    fn from_receipt_usage_mapping() {
        let usage = UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            cache_read_tokens: None,
            cache_write_tokens: None,
            request_units: None,
            estimated_cost_usd: None,
        };
        let events = vec![make_event(AgentEventKind::AssistantMessage {
            text: "hi".into(),
        })];
        let receipt = mock_receipt_with_usage(events, usage);
        let wo = WorkOrderBuilder::new("test").model("gpt-4o").build();
        let resp = from_receipt(&receipt, &wo);

        let u = resp.usage.unwrap();
        assert_eq!(u.prompt_tokens, 100);
        assert_eq!(u.completion_tokens, 50);
        assert_eq!(u.total_tokens, 150);
    }

    // 20
    #[test]
    fn from_receipt_empty_trace() {
        let receipt = mock_receipt(vec![]);
        let wo = WorkOrderBuilder::new("test").model("gpt-4o").build();
        let resp = from_receipt(&receipt, &wo);

        assert!(resp.choices[0].message.content.is_none());
        assert!(resp.choices[0].message.tool_calls.is_none());
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    // 21
    #[test]
    fn from_receipt_id_format() {
        let receipt = mock_receipt(vec![]);
        let wo = WorkOrderBuilder::new("test").model("gpt-4o").build();
        let resp = from_receipt(&receipt, &wo);
        assert!(resp.id.starts_with("chatcmpl-"));
    }

    // 22
    #[test]
    fn from_receipt_model_from_work_order() {
        let receipt = mock_receipt(vec![]);
        let wo = WorkOrderBuilder::new("test").model("o3-mini").build();
        let resp = from_receipt(&receipt, &wo);
        assert_eq!(resp.model, "o3-mini");
    }

    // 23
    #[test]
    fn from_receipt_default_model_when_none() {
        let receipt = mock_receipt(vec![]);
        let wo = WorkOrderBuilder::new("test").build();
        let resp = from_receipt(&receipt, &wo);
        assert_eq!(resp.model, "gpt-4o");
    }

    // 24
    #[test]
    fn from_receipt_multiple_tool_calls() {
        let events = vec![
            make_event(AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("call_1".into()),
                parent_tool_use_id: None,
                input: json!({"path": "a.rs"}),
            }),
            make_event(AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("call_2".into()),
                parent_tool_use_id: None,
                input: json!({"path": "b.rs"}),
            }),
        ];
        let receipt = mock_receipt(events);
        let wo = WorkOrderBuilder::new("test").model("gpt-4o").build();
        let resp = from_receipt(&receipt, &wo);

        let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tcs.len(), 2);
        assert_eq!(tcs[0].id, "call_1");
        assert_eq!(tcs[1].id, "call_2");
    }

    // 25
    #[test]
    fn from_receipt_text_plus_tool_call() {
        let events = vec![
            make_event(AgentEventKind::AssistantMessage {
                text: "Let me check.".into(),
            }),
            make_event(AgentEventKind::ToolCall {
                tool_name: "ls".into(),
                tool_use_id: Some("call_ls".into()),
                parent_tool_use_id: None,
                input: json!({}),
            }),
        ];
        let receipt = mock_receipt(events);
        let wo = WorkOrderBuilder::new("test").model("gpt-4o").build();
        let resp = from_receipt(&receipt, &wo);

        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Let me check.")
        );
        assert!(resp.choices[0].message.tool_calls.is_some());
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
    }

    // ═══════════════════════════════════════════════════════════════════
    // from_agent_event tests
    // ═══════════════════════════════════════════════════════════════════

    // 26
    #[test]
    fn from_agent_event_text_delta() {
        let event = make_event(AgentEventKind::AssistantDelta {
            text: "Hello".into(),
        });
        let chunk = from_agent_event(&event, "gpt-4o", "chunk-1").unwrap();

        assert_eq!(chunk.object, "chat.completion.chunk");
        assert_eq!(chunk.choices[0].delta.content.as_deref(), Some("Hello"));
        assert!(chunk.choices[0].finish_reason.is_none());
    }

    // 27
    #[test]
    fn from_agent_event_assistant_message() {
        let event = make_event(AgentEventKind::AssistantMessage {
            text: "Full msg".into(),
        });
        let chunk = from_agent_event(&event, "gpt-4o", "chunk-1").unwrap();

        assert_eq!(chunk.choices[0].delta.role.as_deref(), Some("assistant"));
        assert_eq!(chunk.choices[0].delta.content.as_deref(), Some("Full msg"));
    }

    // 28
    #[test]
    fn from_agent_event_tool_call() {
        let event = make_event(AgentEventKind::ToolCall {
            tool_name: "search".into(),
            tool_use_id: Some("call_s1".into()),
            parent_tool_use_id: None,
            input: json!({"q": "rust"}),
        });
        let chunk = from_agent_event(&event, "gpt-4o", "chunk-1").unwrap();

        let tc = &chunk.choices[0].delta.tool_calls.as_ref().unwrap()[0];
        assert_eq!(
            tc.function.as_ref().unwrap().name.as_deref(),
            Some("search")
        );
        assert!(
            tc.function
                .as_ref()
                .unwrap()
                .arguments
                .as_ref()
                .unwrap()
                .contains("rust")
        );
        assert_eq!(tc.id.as_deref(), Some("call_s1"));
        assert_eq!(tc.call_type.as_deref(), Some("function"));
    }

    // 29
    #[test]
    fn from_agent_event_run_completed() {
        let event = make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        });
        let chunk = from_agent_event(&event, "gpt-4o", "chunk-1").unwrap();

        assert_eq!(chunk.choices[0].finish_reason.as_deref(), Some("stop"));
        assert!(chunk.choices[0].delta.content.is_none());
    }

    // 30
    #[test]
    fn from_agent_event_error() {
        let event = make_event(AgentEventKind::Error {
            message: "timeout".into(),
            error_code: None,
        });
        let chunk = from_agent_event(&event, "gpt-4o", "chunk-1").unwrap();

        assert_eq!(chunk.choices[0].finish_reason.as_deref(), Some("stop"));
        let content = chunk.choices[0].delta.content.as_deref().unwrap();
        assert!(content.contains("timeout"));
    }

    // 31
    #[test]
    fn from_agent_event_file_changed_returns_none() {
        let event = make_event(AgentEventKind::FileChanged {
            path: "foo.rs".into(),
            summary: "modified".into(),
        });
        assert!(from_agent_event(&event, "gpt-4o", "chunk-1").is_none());
    }

    // 32
    #[test]
    fn from_agent_event_command_executed_returns_none() {
        let event = make_event(AgentEventKind::CommandExecuted {
            command: "ls".into(),
            exit_code: Some(0),
            output_preview: None,
        });
        assert!(from_agent_event(&event, "gpt-4o", "chunk-1").is_none());
    }

    // 33
    #[test]
    fn from_agent_event_warning_returns_none() {
        let event = make_event(AgentEventKind::Warning {
            message: "slow".into(),
        });
        assert!(from_agent_event(&event, "gpt-4o", "chunk-1").is_none());
    }

    // 34
    #[test]
    fn from_agent_event_run_started_returns_none() {
        let event = make_event(AgentEventKind::RunStarted {
            message: "starting".into(),
        });
        assert!(from_agent_event(&event, "gpt-4o", "chunk-1").is_none());
    }

    // 35
    #[test]
    fn from_agent_event_tool_result_returns_none() {
        let event = make_event(AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("call_1".into()),
            output: json!("contents"),
            is_error: false,
        });
        assert!(from_agent_event(&event, "gpt-4o", "chunk-1").is_none());
    }

    // ═══════════════════════════════════════════════════════════════════
    // Helper function tests
    // ═══════════════════════════════════════════════════════════════════

    // 36
    #[test]
    fn usage_from_normalized_computes_total() {
        let usage = UsageNormalized {
            input_tokens: Some(200),
            output_tokens: Some(100),
            cache_read_tokens: None,
            cache_write_tokens: None,
            request_units: None,
            estimated_cost_usd: None,
        };
        let u = usage_from_normalized(&usage);
        assert_eq!(u.prompt_tokens, 200);
        assert_eq!(u.completion_tokens, 100);
        assert_eq!(u.total_tokens, 300);
    }

    // 37
    #[test]
    fn usage_from_normalized_defaults_to_zero() {
        let usage = UsageNormalized::default();
        let u = usage_from_normalized(&usage);
        assert_eq!(u.prompt_tokens, 0);
        assert_eq!(u.completion_tokens, 0);
        assert_eq!(u.total_tokens, 0);
    }

    // 38
    #[test]
    fn role_to_str_all_roles() {
        assert_eq!(
            role_to_str(&ChatMessage::System {
                content: "x".into()
            }),
            "system"
        );
        assert_eq!(
            role_to_str(&ChatMessage::User {
                content: MessageContent::Text("x".into())
            }),
            "user"
        );
        assert_eq!(
            role_to_str(&ChatMessage::Assistant {
                content: Some("x".into()),
                tool_calls: None
            }),
            "assistant"
        );
        assert_eq!(
            role_to_str(&ChatMessage::Tool {
                content: "x".into(),
                tool_call_id: "id".into()
            }),
            "tool"
        );
    }

    // 39
    #[test]
    fn message_content_to_string_text() {
        let content = MessageContent::Text("hello".into());
        assert_eq!(message_content_to_string(&content), "hello");
    }

    // 40
    #[test]
    fn message_content_to_string_parts() {
        let content = MessageContent::Parts(vec![
            ContentPart::Text {
                text: "Hello ".into(),
            },
            ContentPart::Text {
                text: "world".into(),
            },
        ]);
        assert_eq!(message_content_to_string(&content), "Hello world");
    }

    // 41
    #[test]
    fn message_content_to_string_parts_skips_images() {
        let content = MessageContent::Parts(vec![
            ContentPart::Text {
                text: "See: ".into(),
            },
            ContentPart::ImageUrl {
                image_url: ImageUrl {
                    url: "https://example.com/img.png".into(),
                    detail: None,
                },
            },
        ]);
        assert_eq!(message_content_to_string(&content), "See: ");
    }

    // 42
    #[test]
    fn count_roles_basic() {
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![
                ChatMessage::System {
                    content: "sys".into(),
                },
                ChatMessage::User {
                    content: MessageContent::Text("u1".into()),
                },
                ChatMessage::Assistant {
                    content: Some("a1".into()),
                    tool_calls: None,
                },
                ChatMessage::User {
                    content: MessageContent::Text("u2".into()),
                },
            ],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
        };
        let counts = count_roles(&req);
        assert_eq!(counts["system"], 1);
        assert_eq!(counts["user"], 2);
        assert_eq!(counts["assistant"], 1);
    }

    // 43
    #[test]
    fn make_stop_chunk_has_stop_reason() {
        let chunk = make_stop_chunk("gpt-4o", "chunk-end");
        assert_eq!(chunk.choices[0].finish_reason.as_deref(), Some("stop"));
        assert_eq!(chunk.model, "gpt-4o");
        assert_eq!(chunk.id, "chunk-end");
    }

    // 44
    #[test]
    fn tools_to_json_empty_list() {
        let tools: Vec<Tool> = vec![];
        let v = tools_to_json(&tools);
        assert!(v.is_array());
        assert_eq!(v.as_array().unwrap().len(), 0);
    }

    // 45
    #[test]
    fn tools_to_json_single_tool() {
        let tools = vec![Tool {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "read_file".into(),
                description: "Read a file".into(),
                parameters: json!({"type": "object"}),
            },
        }];
        let v = tools_to_json(&tools);
        assert_eq!(v.as_array().unwrap().len(), 1);
    }

    // ═══════════════════════════════════════════════════════════════════
    // Roundtrip / integration tests
    // ═══════════════════════════════════════════════════════════════════

    // 46
    #[test]
    fn roundtrip_simple_message() {
        let req = minimal_request();
        let wo = to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));

        let events = vec![make_event(AgentEventKind::AssistantMessage {
            text: "Hi there!".into(),
        })];
        let receipt = mock_receipt(events);
        let resp = from_receipt(&receipt, &wo);

        assert_eq!(resp.model, "gpt-4o");
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Hi there!")
        );
    }

    // 47
    #[test]
    fn roundtrip_with_tools() {
        let req = request_with_tools();
        let wo = to_work_order(&req);
        assert!(wo.config.vendor.contains_key("tools"));

        let events = vec![make_event(AgentEventKind::ToolCall {
            tool_name: "get_weather".into(),
            tool_use_id: Some("call_w1".into()),
            parent_tool_use_id: None,
            input: json!({"location": "SF"}),
        })];
        let receipt = mock_receipt(events);
        let resp = from_receipt(&receipt, &wo);

        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
        let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tcs[0].function.name, "get_weather");
    }

    // 48
    #[test]
    fn from_agent_event_preserves_model() {
        let event = make_event(AgentEventKind::AssistantDelta { text: "hi".into() });
        let chunk = from_agent_event(&event, "o3-mini", "c1").unwrap();
        assert_eq!(chunk.model, "o3-mini");
    }

    // 49
    #[test]
    fn from_agent_event_preserves_chunk_id() {
        let event = make_event(AgentEventKind::AssistantDelta { text: "hi".into() });
        let chunk = from_agent_event(&event, "gpt-4o", "chatcmpl-xyz").unwrap();
        assert_eq!(chunk.id, "chatcmpl-xyz");
    }

    // 50
    #[test]
    fn to_work_order_multipart_content() {
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![ChatMessage::User {
                content: MessageContent::Parts(vec![
                    ContentPart::Text {
                        text: "Describe ".into(),
                    },
                    ContentPart::Text {
                        text: "this image".into(),
                    },
                ]),
            }],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
        };
        let wo = to_work_order(&req);
        assert_eq!(wo.task, "Describe this image");
    }

    // 51
    #[test]
    fn from_receipt_tool_call_without_id_generates_one() {
        let events = vec![make_event(AgentEventKind::ToolCall {
            tool_name: "search".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({}),
        })];
        let receipt = mock_receipt(events);
        let wo = WorkOrderBuilder::new("test").model("gpt-4o").build();
        let resp = from_receipt(&receipt, &wo);

        let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert!(tcs[0].id.starts_with("call_"));
    }

    // 52
    #[test]
    fn from_receipt_created_timestamp_set() {
        let receipt = mock_receipt(vec![]);
        let wo = WorkOrderBuilder::new("test").model("gpt-4o").build();
        let resp = from_receipt(&receipt, &wo);
        assert!(resp.created > 0);
    }

    // 53
    #[test]
    fn from_agent_event_tool_call_no_id() {
        let event = make_event(AgentEventKind::ToolCall {
            tool_name: "ls".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({}),
        });
        let chunk = from_agent_event(&event, "gpt-4o", "c1").unwrap();
        let tc = &chunk.choices[0].delta.tool_calls.as_ref().unwrap()[0];
        assert!(tc.id.is_none());
    }
}
