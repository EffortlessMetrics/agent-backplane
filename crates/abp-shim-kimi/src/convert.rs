// SPDX-License-Identifier: MIT OR Apache-2.0
//! Conversion layer between Kimi shim types and ABP core types.
//!
//! Provides the main conversion functions that form the core of the drop-in
//! Kimi API replacement:
//!
//! - [`request_to_ir`] — Kimi request → ABP IR
//! - [`request_to_work_order`] — Kimi request → ABP `WorkOrder`
//! - [`receipt_to_response`] — ABP `Receipt` → Kimi response
//! - [`events_to_stream_chunks`] — ABP streaming events → Kimi SSE chunks
//! - [`response_to_ir`] — Kimi response → ABP IR
//! - [`ir_to_messages`] / [`messages_to_ir`] — roundtrip between IR and shim messages
//! - [`ir_usage_to_usage`] — IR usage → shim [`Usage`]

use abp_core::ir::{IrConversation, IrRole, IrUsage};
use abp_core::{
    AgentEvent, AgentEventKind, Receipt, RuntimeConfig, UsageNormalized, WorkOrder,
    WorkOrderBuilder,
};
use abp_kimi_sdk::dialect::{
    KimiChoice, KimiChunk, KimiChunkChoice, KimiChunkDelta, KimiFunctionCall, KimiMessage,
    KimiRequest, KimiResponse, KimiResponseMessage, KimiToolCall, KimiUsage,
};
use abp_kimi_sdk::lowering;
use chrono::Utc;

use crate::types::{Message, Usage, to_kimi_message};

// ── Conversion: request → IR → WorkOrder ────────────────────────────────

/// Convert a [`KimiRequest`] into an [`IrConversation`].
pub fn request_to_ir(request: &KimiRequest) -> IrConversation {
    lowering::to_ir(&request.messages)
}

/// Convert a [`KimiRequest`] into an ABP [`WorkOrder`].
///
/// Maps:
/// - `messages` → `work_order.task` (extracted from last user message)
/// - `model` → `work_order.config.model`
/// - `temperature`, `max_tokens` → `work_order.config.vendor`
pub fn request_to_work_order(request: &KimiRequest) -> WorkOrder {
    let conv = request_to_ir(request);
    let task = conv
        .messages
        .iter()
        .rev()
        .find(|m| m.role == IrRole::User)
        .map(|m| m.text_content())
        .unwrap_or_else(|| "kimi completion".into());

    let mut builder = WorkOrderBuilder::new(task).model(request.model.clone());

    let mut vendor = std::collections::BTreeMap::new();
    if let Some(temp) = request.temperature {
        vendor.insert("temperature".to_string(), serde_json::Value::from(temp));
    }
    if let Some(max) = request.max_tokens {
        vendor.insert("max_tokens".to_string(), serde_json::Value::from(max));
    }
    let config = RuntimeConfig {
        model: Some(request.model.clone()),
        vendor,
        ..Default::default()
    };
    builder = builder.config(config);

    builder.build()
}

// ── Conversion: Receipt → KimiResponse ──────────────────────────────────

/// Build a [`KimiResponse`] from a [`Receipt`] and the original model name.
///
/// Walks the receipt trace to reconstruct the assistant message content
/// and any tool calls. Extracts usage from `receipt.usage`.
pub fn receipt_to_response(receipt: &Receipt, model: &str) -> KimiResponse {
    let mut content: Option<String> = None;
    let mut tool_calls: Vec<KimiToolCall> = Vec::new();
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
                tool_calls.push(KimiToolCall {
                    id: tool_use_id
                        .clone()
                        .unwrap_or_else(|| format!("call_{}", uuid::Uuid::new_v4())),
                    call_type: "function".into(),
                    function: KimiFunctionCall {
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

    let message = KimiResponseMessage {
        role: "assistant".into(),
        content,
        tool_calls: if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        },
    };

    let usage = usage_from_receipt(&receipt.usage);

    KimiResponse {
        id: format!("cmpl-{}", receipt.meta.run_id),
        model: model.to_string(),
        choices: vec![KimiChoice {
            index: 0,
            message,
            finish_reason: Some(finish_reason),
        }],
        usage: Some(usage),
        refs: None,
    }
}

/// Convert normalized usage to Kimi-style usage.
fn usage_from_receipt(usage: &UsageNormalized) -> KimiUsage {
    let prompt = usage.input_tokens.unwrap_or(0);
    let completion = usage.output_tokens.unwrap_or(0);
    KimiUsage {
        prompt_tokens: prompt,
        completion_tokens: completion,
        total_tokens: prompt + completion,
    }
}

/// Build [`KimiChunk`]s from a sequence of [`AgentEvent`]s.
///
/// Each delta/message event produces a chunk; a final `"stop"` chunk is
/// always appended.
pub fn events_to_stream_chunks(events: &[AgentEvent], model: &str) -> Vec<KimiChunk> {
    let run_id = format!("cmpl-{}", uuid::Uuid::new_v4());
    let created = Utc::now().timestamp() as u64;
    let mut chunks = Vec::new();

    for event in events {
        match &event.kind {
            AgentEventKind::AssistantDelta { text } => {
                chunks.push(KimiChunk {
                    id: run_id.clone(),
                    object: "chat.completion.chunk".into(),
                    created,
                    model: model.to_string(),
                    choices: vec![KimiChunkChoice {
                        index: 0,
                        delta: KimiChunkDelta {
                            role: None,
                            content: Some(text.clone()),
                            tool_calls: None,
                        },
                        finish_reason: None,
                    }],
                    usage: None,
                    refs: None,
                });
            }
            AgentEventKind::AssistantMessage { text } => {
                chunks.push(KimiChunk {
                    id: run_id.clone(),
                    object: "chat.completion.chunk".into(),
                    created,
                    model: model.to_string(),
                    choices: vec![KimiChunkChoice {
                        index: 0,
                        delta: KimiChunkDelta {
                            role: Some("assistant".into()),
                            content: Some(text.clone()),
                            tool_calls: None,
                        },
                        finish_reason: None,
                    }],
                    usage: None,
                    refs: None,
                });
            }
            _ => {}
        }
    }

    // Final stop chunk
    chunks.push(KimiChunk {
        id: run_id,
        object: "chat.completion.chunk".into(),
        created,
        model: model.to_string(),
        choices: vec![KimiChunkChoice {
            index: 0,
            delta: KimiChunkDelta::default(),
            finish_reason: Some("stop".into()),
        }],
        usage: None,
        refs: None,
    });

    chunks
}

/// Convert a [`KimiResponse`] into an [`IrConversation`].
pub fn response_to_ir(response: &KimiResponse) -> IrConversation {
    let msgs: Vec<KimiMessage> = response
        .choices
        .iter()
        .map(|c| KimiMessage {
            role: c.message.role.clone(),
            content: c.message.content.clone(),
            tool_call_id: None,
            tool_calls: c.message.tool_calls.clone(),
        })
        .collect();
    lowering::to_ir(&msgs)
}

/// Convert an [`IrConversation`] back to shim [`Message`]s.
pub fn ir_to_messages(conv: &IrConversation) -> Vec<Message> {
    let kimi_msgs = lowering::from_ir(conv);
    kimi_msgs
        .into_iter()
        .map(|m| Message {
            role: m.role,
            content: m.content,
            tool_calls: m.tool_calls,
            tool_call_id: m.tool_call_id,
        })
        .collect()
}

/// Convert shim [`Message`]s to an [`IrConversation`].
pub fn messages_to_ir(messages: &[Message]) -> IrConversation {
    let kimi_msgs: Vec<KimiMessage> = messages
        .iter()
        .map(|m| to_kimi_message(m.clone()))
        .collect();
    lowering::to_ir(&kimi_msgs)
}

/// Convert an [`IrUsage`] to shim [`Usage`].
pub fn ir_usage_to_usage(ir: &IrUsage) -> Usage {
    Usage {
        prompt_tokens: ir.input_tokens,
        completion_tokens: ir.output_tokens,
        total_tokens: ir.total_tokens,
    }
}
