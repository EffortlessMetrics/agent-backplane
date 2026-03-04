// SPDX-License-Identifier: MIT OR Apache-2.0
//! Conversion layer between Codex Responses API types and ABP core types.
//!
//! This module provides the main conversion functions that form the core
//! of the drop-in Codex API replacement:
//!
//! - [`request_to_ir`] — Codex request → ABP IR conversation
//! - [`request_to_work_order`] — Codex request → ABP `WorkOrder`
//! - [`receipt_to_response`] — ABP `Receipt` → Codex response
//! - [`events_to_stream_events`] — ABP streaming events → Codex SSE events
//! - [`response_to_ir`] / [`ir_to_response_items`] — roundtrip through IR

use abp_codex_sdk::dialect::{
    CodexContentPart, CodexRequest, CodexResponse, CodexResponseItem, CodexStreamDelta,
    CodexStreamEvent, CodexUsage,
};
use abp_codex_sdk::lowering;
use abp_core::ir::{IrConversation, IrRole, IrUsage};
use abp_core::{AgentEvent, AgentEventKind, Receipt, UsageNormalized, WorkOrder, WorkOrderBuilder};

use crate::types::Usage;

// ── Conversion: request → IR → WorkOrder ────────────────────────────────

/// Convert a [`CodexRequest`] into an [`IrConversation`].
pub fn request_to_ir(request: &CodexRequest) -> IrConversation {
    lowering::input_to_ir(&request.input)
}

/// Convert a [`CodexRequest`] into an ABP [`WorkOrder`].
///
/// Maps the Codex request fields onto an ABP work order:
/// - `input` → task text (extracted from last user message)
/// - `model` → `work_order.config.model`
/// - `temperature`, `max_output_tokens` → `work_order.config.vendor`
pub fn request_to_work_order(request: &CodexRequest) -> WorkOrder {
    let conv = request_to_ir(request);
    let task = conv
        .messages
        .iter()
        .rev()
        .find(|m| m.role == IrRole::User)
        .map(|m| m.text_content())
        .unwrap_or_else(|| "codex completion".into());

    let mut builder = WorkOrderBuilder::new(task).model(request.model.clone());

    let mut vendor = std::collections::BTreeMap::new();
    if let Some(temp) = request.temperature {
        vendor.insert("temperature".to_string(), serde_json::Value::from(temp));
    }
    if let Some(max) = request.max_output_tokens {
        vendor.insert(
            "max_output_tokens".to_string(),
            serde_json::Value::from(max),
        );
    }
    let config = abp_core::RuntimeConfig {
        model: Some(request.model.clone()),
        vendor,
        ..Default::default()
    };
    builder = builder.config(config);

    builder.build()
}

// ── Conversion: Receipt → CodexResponse ─────────────────────────────────

/// Build a [`CodexResponse`] from a [`Receipt`] and the original model name.
pub fn receipt_to_response(receipt: &Receipt, model: &str) -> CodexResponse {
    let mut output = Vec::new();

    for event in &receipt.trace {
        match &event.kind {
            AgentEventKind::AssistantMessage { text } => {
                output.push(CodexResponseItem::Message {
                    role: "assistant".into(),
                    content: vec![CodexContentPart::OutputText { text: text.clone() }],
                });
            }
            AgentEventKind::AssistantDelta { text } => {
                output.push(CodexResponseItem::Message {
                    role: "assistant".into(),
                    content: vec![CodexContentPart::OutputText { text: text.clone() }],
                });
            }
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                input,
                ..
            } => {
                output.push(CodexResponseItem::FunctionCall {
                    id: tool_use_id
                        .clone()
                        .unwrap_or_else(|| format!("fc_{}", uuid::Uuid::new_v4())),
                    call_id: None,
                    name: tool_name.clone(),
                    arguments: serde_json::to_string(input).unwrap_or_default(),
                });
            }
            AgentEventKind::Error { message, .. } => {
                output.push(CodexResponseItem::Message {
                    role: "assistant".into(),
                    content: vec![CodexContentPart::OutputText {
                        text: format!("Error: {message}"),
                    }],
                });
            }
            _ => {}
        }
    }

    let usage = usage_from_receipt(&receipt.usage);

    CodexResponse {
        id: format!("resp_{}", receipt.meta.run_id),
        model: model.to_string(),
        output,
        usage: Some(usage),
        status: Some("completed".into()),
    }
}

/// Convert normalized usage to Codex-style usage.
fn usage_from_receipt(usage: &UsageNormalized) -> CodexUsage {
    let input = usage.input_tokens.unwrap_or(0);
    let output = usage.output_tokens.unwrap_or(0);
    CodexUsage {
        input_tokens: input,
        output_tokens: output,
        total_tokens: input + output,
    }
}

// ── Conversion: AgentEvents → CodexStreamEvents ─────────────────────────

/// Build [`CodexStreamEvent`]s from a sequence of [`AgentEvent`]s.
pub fn events_to_stream_events(events: &[AgentEvent], model: &str) -> Vec<CodexStreamEvent> {
    let run_id = format!("resp_{}", uuid::Uuid::new_v4());
    let mut stream_events = Vec::new();

    // Initial created event
    stream_events.push(CodexStreamEvent::ResponseCreated {
        response: CodexResponse {
            id: run_id.clone(),
            model: model.to_string(),
            output: vec![],
            usage: None,
            status: Some("in_progress".into()),
        },
    });

    for (i, event) in events.iter().enumerate() {
        match &event.kind {
            AgentEventKind::AssistantDelta { text } => {
                stream_events.push(CodexStreamEvent::OutputItemDelta {
                    output_index: i,
                    delta: CodexStreamDelta::OutputTextDelta { text: text.clone() },
                });
            }
            AgentEventKind::AssistantMessage { text } => {
                let item = CodexResponseItem::Message {
                    role: "assistant".into(),
                    content: vec![CodexContentPart::OutputText { text: text.clone() }],
                };
                stream_events.push(CodexStreamEvent::OutputItemDone {
                    output_index: i,
                    item,
                });
            }
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                input,
                ..
            } => {
                let item = CodexResponseItem::FunctionCall {
                    id: tool_use_id
                        .clone()
                        .unwrap_or_else(|| format!("fc_{}", uuid::Uuid::new_v4())),
                    call_id: None,
                    name: tool_name.clone(),
                    arguments: serde_json::to_string(input).unwrap_or_default(),
                };
                stream_events.push(CodexStreamEvent::OutputItemDone {
                    output_index: i,
                    item,
                });
            }
            _ => {}
        }
    }

    // Final completed event
    stream_events.push(CodexStreamEvent::ResponseCompleted {
        response: CodexResponse {
            id: run_id,
            model: model.to_string(),
            output: vec![],
            usage: None,
            status: Some("completed".into()),
        },
    });

    stream_events
}

// ── Conversion: IR roundtrip ────────────────────────────────────────────

/// Convert a [`CodexResponse`] into an [`IrConversation`].
pub fn response_to_ir(response: &CodexResponse) -> IrConversation {
    lowering::to_ir(&response.output)
}

/// Convert an [`IrConversation`] back into Codex response items.
pub fn ir_to_response_items(conv: &IrConversation) -> Vec<CodexResponseItem> {
    lowering::from_ir(conv)
}

/// Convert an [`IrUsage`] to shim [`Usage`].
pub fn ir_usage_to_usage(ir: &IrUsage) -> Usage {
    Usage {
        input_tokens: ir.input_tokens,
        output_tokens: ir.output_tokens,
        total_tokens: ir.total_tokens,
    }
}
