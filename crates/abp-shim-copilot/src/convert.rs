// SPDX-License-Identifier: MIT OR Apache-2.0
//! Conversion layer between Copilot SDK types and ABP core types.
//!
//! Provides functions for mapping between the Copilot wire format and ABP's
//! intermediate representation:
//!
//! - [`request_to_ir`] / [`messages_to_ir`] — Copilot messages → IR
//! - [`request_to_work_order`] — Copilot request → ABP `WorkOrder`
//! - [`receipt_to_response`] — ABP `Receipt` → Copilot response
//! - [`events_to_stream_events`] — ABP events → Copilot SSE stream events
//! - [`response_to_ir`] / [`ir_to_messages`] — roundtrip helpers

use abp_copilot_sdk::dialect::{
    CopilotError, CopilotFunctionCall, CopilotMessage, CopilotResponse, CopilotStreamEvent,
};
use abp_copilot_sdk::lowering;
use abp_core::ir::{IrConversation, IrRole, IrUsage};
use abp_core::{AgentEvent, AgentEventKind, Receipt, WorkOrder, WorkOrderBuilder};

use crate::types::{Message, to_copilot_message};

// ── Conversion: request → IR → WorkOrder ────────────────────────────────

/// Convert a [`CopilotRequest`](abp_copilot_sdk::dialect::CopilotRequest) into
/// an [`IrConversation`].
pub fn request_to_ir(request: &abp_copilot_sdk::dialect::CopilotRequest) -> IrConversation {
    lowering::to_ir(&request.messages)
}

/// Convert a [`CopilotRequest`](abp_copilot_sdk::dialect::CopilotRequest) into
/// an ABP [`WorkOrder`].
pub fn request_to_work_order(request: &abp_copilot_sdk::dialect::CopilotRequest) -> WorkOrder {
    let conv = request_to_ir(request);
    let task = conv
        .messages
        .iter()
        .rev()
        .find(|m| m.role == IrRole::User)
        .map(|m| m.text_content())
        .unwrap_or_else(|| "copilot completion".into());

    let mut builder = WorkOrderBuilder::new(task).model(request.model.clone());

    let config = abp_core::RuntimeConfig {
        model: Some(request.model.clone()),
        ..Default::default()
    };
    builder = builder.config(config);

    builder.build()
}

// ── Conversion: Receipt → CopilotResponse ───────────────────────────────

/// Build a [`CopilotResponse`] from a [`Receipt`] and the original model name.
pub fn receipt_to_response(receipt: &Receipt, _model: &str) -> CopilotResponse {
    let mut message = String::new();
    let mut errors = Vec::new();
    let mut function_call: Option<CopilotFunctionCall> = None;

    for event in &receipt.trace {
        match &event.kind {
            AgentEventKind::AssistantMessage { text } => {
                message = text.clone();
            }
            AgentEventKind::AssistantDelta { text } => {
                message.push_str(text);
            }
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                input,
                ..
            } => {
                function_call = Some(CopilotFunctionCall {
                    name: tool_name.clone(),
                    arguments: serde_json::to_string(input).unwrap_or_default(),
                    id: tool_use_id.clone(),
                });
            }
            AgentEventKind::Error {
                message: msg,
                error_code,
            } => {
                errors.push(CopilotError {
                    error_type: "backend_error".into(),
                    message: msg.clone(),
                    code: error_code.as_ref().map(|c| c.to_string()),
                    identifier: None,
                });
            }
            _ => {}
        }
    }

    CopilotResponse {
        message,
        copilot_references: vec![],
        copilot_errors: errors,
        copilot_confirmation: None,
        function_call,
    }
}

/// Build [`CopilotStreamEvent`]s from a sequence of [`AgentEvent`]s.
pub fn events_to_stream_events(events: &[AgentEvent], _model: &str) -> Vec<CopilotStreamEvent> {
    let mut stream_events = Vec::new();

    // Initial references event (empty)
    stream_events.push(CopilotStreamEvent::CopilotReferences { references: vec![] });

    for event in events {
        match &event.kind {
            AgentEventKind::AssistantDelta { text } => {
                stream_events.push(CopilotStreamEvent::TextDelta { text: text.clone() });
            }
            AgentEventKind::AssistantMessage { text } => {
                stream_events.push(CopilotStreamEvent::TextDelta { text: text.clone() });
            }
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                input,
                ..
            } => {
                stream_events.push(CopilotStreamEvent::FunctionCall {
                    function_call: CopilotFunctionCall {
                        name: tool_name.clone(),
                        arguments: serde_json::to_string(input).unwrap_or_default(),
                        id: tool_use_id.clone(),
                    },
                });
            }
            AgentEventKind::Error { message, .. } => {
                stream_events.push(CopilotStreamEvent::CopilotErrors {
                    errors: vec![CopilotError {
                        error_type: "backend_error".into(),
                        message: message.clone(),
                        code: None,
                        identifier: None,
                    }],
                });
            }
            _ => {}
        }
    }

    // Final done event
    stream_events.push(CopilotStreamEvent::Done {});

    stream_events
}

// ── Conversion: CopilotResponse → IR ────────────────────────────────────

/// Convert a [`CopilotResponse`] into an [`IrConversation`].
pub fn response_to_ir(response: &CopilotResponse) -> IrConversation {
    if response.message.is_empty() {
        return IrConversation::from_messages(vec![]);
    }
    let msgs = vec![CopilotMessage {
        role: "assistant".into(),
        content: response.message.clone(),
        name: None,
        copilot_references: response.copilot_references.clone(),
    }];
    lowering::to_ir(&msgs)
}

/// Convert an [`IrConversation`] back to shim [`Message`]s.
pub fn ir_to_messages(conv: &IrConversation) -> Vec<Message> {
    let copilot_msgs = lowering::from_ir(conv);
    copilot_msgs
        .into_iter()
        .map(|m| Message {
            role: m.role,
            content: m.content,
            name: m.name,
            copilot_references: m.copilot_references,
            copilot_confirmations: Vec::new(),
        })
        .collect()
}

/// Convert shim [`Message`]s to an [`IrConversation`].
pub fn messages_to_ir(messages: &[Message]) -> IrConversation {
    let copilot_msgs: Vec<CopilotMessage> = messages
        .iter()
        .map(|m| to_copilot_message(m.clone()))
        .collect();
    lowering::to_ir(&copilot_msgs)
}

/// Convert an [`IrUsage`] to a simple usage tuple `(input, output, total)`.
pub fn ir_usage_to_tuple(ir: &IrUsage) -> (u64, u64, u64) {
    (ir.input_tokens, ir.output_tokens, ir.total_tokens)
}

// ── Named translation functions ─────────────────────────────────────────

/// Translate a [`CopilotRequest`](abp_copilot_sdk::dialect::CopilotRequest)
/// into an ABP [`WorkOrder`].
///
/// Named alias for [`request_to_work_order`] following the ABP SDK shim
/// convention of `translate_to_work_order` / `translate_from_receipt` pairs.
pub fn translate_to_work_order(request: &abp_copilot_sdk::dialect::CopilotRequest) -> WorkOrder {
    request_to_work_order(request)
}

/// Translate an ABP [`Receipt`] into a [`CopilotResponse`].
///
/// Named alias for [`receipt_to_response`] following the ABP SDK shim convention.
pub fn translate_from_receipt(receipt: &Receipt, model: &str) -> CopilotResponse {
    receipt_to_response(receipt, model)
}
