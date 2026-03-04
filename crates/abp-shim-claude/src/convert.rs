// SPDX-License-Identifier: MIT OR Apache-2.0
//! Conversion layer between Anthropic Claude Messages API types and ABP core types.
//!
//! This module provides the core functions for the Claude shim's drop-in
//! replacement for the Anthropic Messages API:
//!
//! - [`to_work_order`] — Convert a Claude `MessagesRequest` into an ABP `WorkOrder`.
//! - [`from_receipt`] — Convert an ABP `Receipt` back into a Claude `MessagesResponse`.
//! - [`from_agent_event`] — Convert streaming `AgentEvent`s to Claude SSE `StreamEvent`s.

use std::collections::BTreeMap;

use abp_core::{AgentEvent, AgentEventKind, Receipt, RuntimeConfig, WorkOrder, WorkOrderBuilder};
use abp_sdk_types::Dialect;
use serde_json::json;

use crate::types::{
    ClaudeContent, ClaudeTool, ClaudeUsage, ContentBlock, ErrorResponse, MessageDeltaBody,
    MessagesRequest, MessagesResponse, StreamDelta, StreamEvent,
};

// ---------------------------------------------------------------------------
// Request → WorkOrder
// ---------------------------------------------------------------------------

/// Convert a Claude Messages API request into an ABP [`WorkOrder`].
///
/// Maps the Claude-specific fields into the canonical ABP contract:
/// - Messages are inspected to extract the last user text as the task description.
/// - The system prompt is stored in `vendor["system"]`.
/// - Model, temperature, top_p, max_tokens, and stream are preserved in
///   `config.vendor`.
/// - Tools are stored as a JSON array in `vendor["tools"]`.
/// - The dialect is recorded as `vendor["dialect"] = "claude"`.
#[must_use]
pub fn to_work_order(req: &MessagesRequest) -> WorkOrder {
    let task = extract_task(req);

    let mut vendor = BTreeMap::new();
    vendor.insert(
        "dialect".to_string(),
        serde_json::to_value(Dialect::Claude).unwrap_or_default(),
    );
    vendor.insert("max_tokens".to_string(), json!(req.max_tokens));

    if let Some(ref system) = req.system {
        vendor.insert("system".to_string(), json!(system));
    }
    if let Some(temp) = req.temperature {
        vendor.insert("temperature".to_string(), json!(temp));
    }
    if let Some(top_p) = req.top_p {
        vendor.insert("top_p".to_string(), json!(top_p));
    }
    if let Some(top_k) = req.top_k {
        vendor.insert("top_k".to_string(), json!(top_k));
    }
    if let Some(stream) = req.stream {
        vendor.insert("stream".to_string(), json!(stream));
    }
    if let Some(ref tools) = req.tools {
        vendor.insert(
            "tools".to_string(),
            serde_json::to_value(tools).unwrap_or_default(),
        );
    }
    if let Some(ref tool_choice) = req.tool_choice {
        vendor.insert(
            "tool_choice".to_string(),
            serde_json::to_value(tool_choice).unwrap_or_default(),
        );
    }
    if let Some(ref stop_sequences) = req.stop_sequences {
        vendor.insert(
            "stop_sequences".to_string(),
            serde_json::to_value(stop_sequences).unwrap_or_default(),
        );
    }
    if let Some(ref thinking) = req.thinking {
        vendor.insert(
            "thinking".to_string(),
            serde_json::to_value(thinking).unwrap_or_default(),
        );
    }

    // Store messages as structured JSON for faithful round-tripping.
    vendor.insert(
        "messages".to_string(),
        serde_json::to_value(&req.messages).unwrap_or_default(),
    );

    let config = RuntimeConfig {
        model: Some(req.model.clone()),
        vendor,
        ..Default::default()
    };

    WorkOrderBuilder::new(task)
        .model(&req.model)
        .config(config)
        .build()
}

// ---------------------------------------------------------------------------
// Receipt → MessagesResponse
// ---------------------------------------------------------------------------

/// Convert an ABP [`Receipt`] back into a Claude [`MessagesResponse`].
///
/// Reconstructs the Claude API response shape from receipt data:
/// - Text content from `AssistantMessage` trace events becomes `ContentBlock::Text`.
/// - Tool calls from `ToolCall` trace events become `ContentBlock::ToolUse`.
/// - Usage is extracted from `receipt.usage_raw` (Claude `input_tokens`/`output_tokens`).
/// - Stop reason is inferred from the trace (tool_use if tools were called, else end_turn).
#[must_use]
pub fn from_receipt(receipt: &Receipt, wo: &WorkOrder) -> MessagesResponse {
    let mut content = Vec::new();
    let mut stop_reason: Option<String> = None;

    for event in &receipt.trace {
        match &event.kind {
            AgentEventKind::AssistantMessage { text } => {
                let is_thinking = event
                    .ext
                    .as_ref()
                    .and_then(|e| e.get("thinking"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                if is_thinking {
                    let signature = event
                        .ext
                        .as_ref()
                        .and_then(|e| e.get("signature"))
                        .and_then(|v| v.as_str())
                        .map(String::from);
                    content.push(ContentBlock::Thinking {
                        thinking: text.clone(),
                        signature,
                    });
                } else {
                    content.push(ContentBlock::Text { text: text.clone() });
                }
            }
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                input,
                ..
            } => {
                content.push(ContentBlock::ToolUse {
                    id: tool_use_id.clone().unwrap_or_default(),
                    name: tool_name.clone(),
                    input: input.clone(),
                });
                stop_reason = Some("tool_use".to_string());
            }
            AgentEventKind::RunCompleted { .. } if stop_reason.is_none() => {
                stop_reason = Some("end_turn".to_string());
            }
            _ => {}
        }
    }

    if stop_reason.is_none() && !content.is_empty() {
        stop_reason = Some("end_turn".to_string());
    }

    let usage = usage_from_raw(&receipt.usage_raw);
    let model = wo
        .config
        .model
        .clone()
        .unwrap_or_else(|| "claude-sonnet-4-20250514".to_string());

    MessagesResponse {
        id: format!("msg_{}", receipt.meta.run_id.as_simple()),
        type_field: "message".to_string(),
        role: "assistant".to_string(),
        content,
        model,
        stop_reason,
        usage,
    }
}

// ---------------------------------------------------------------------------
// AgentEvent → StreamEvent
// ---------------------------------------------------------------------------

/// Convert a single ABP [`AgentEvent`] into an optional Claude [`StreamEvent`].
///
/// Mapping:
/// - `AssistantDelta` → `ContentBlockDelta` with `TextDelta`
/// - `AssistantMessage` with `ext.thinking` → `ContentBlockDelta` with `ThinkingDelta`
/// - `AssistantMessage` → `ContentBlockDelta` with `TextDelta`
/// - `ToolCall` → `ContentBlockStart` with a `ToolUse` content block
/// - `RunCompleted` → `MessageDelta` with `stop_reason = "end_turn"`
/// - `Error` → `Error` stream event
/// - Other event kinds return `None`.
#[must_use]
pub fn from_agent_event(event: &AgentEvent) -> Option<StreamEvent> {
    match &event.kind {
        AgentEventKind::AssistantDelta { text } => Some(StreamEvent::ContentBlockDelta {
            index: 0,
            delta: StreamDelta::TextDelta { text: text.clone() },
        }),
        AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            input,
            ..
        } => Some(StreamEvent::ContentBlockStart {
            index: 0,
            content_block: ContentBlock::ToolUse {
                id: tool_use_id.clone().unwrap_or_default(),
                name: tool_name.clone(),
                input: input.clone(),
            },
        }),
        AgentEventKind::RunCompleted { .. } => Some(StreamEvent::MessageDelta {
            delta: MessageDeltaBody {
                stop_reason: Some("end_turn".to_string()),
                stop_sequence: None,
            },
            usage: None,
        }),
        AgentEventKind::AssistantMessage { text } => {
            let is_thinking = event
                .ext
                .as_ref()
                .and_then(|e| e.get("thinking"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            if is_thinking {
                Some(StreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: StreamDelta::ThinkingDelta {
                        thinking: text.clone(),
                    },
                })
            } else {
                Some(StreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: StreamDelta::TextDelta { text: text.clone() },
                })
            }
        }
        AgentEventKind::Error { message, .. } => Some(StreamEvent::Error {
            error: ErrorResponse {
                error_type: "api_error".to_string(),
                message: message.clone(),
            },
        }),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Extract the task description from a [`MessagesRequest`].
///
/// Uses the text content of the last user message; falls back to the system
/// prompt or a default string.
#[must_use]
pub fn extract_task(req: &MessagesRequest) -> String {
    // Try the last user message.
    for msg in req.messages.iter().rev() {
        if msg.role == "user" {
            if let Some(text) = content_to_text(&msg.content) {
                return text;
            }
        }
    }
    // Fall back to system prompt.
    if let Some(ref sys) = req.system {
        return sys.clone();
    }
    "Claude shim request".to_string()
}

/// Extract plain text from a [`ClaudeContent`] value.
///
/// For `Text` variants returns the string directly. For `Blocks` variants,
/// concatenates all text blocks.
#[must_use]
pub fn content_to_text(content: &ClaudeContent) -> Option<String> {
    match content {
        ClaudeContent::Text(s) => {
            if s.is_empty() {
                None
            } else {
                Some(s.clone())
            }
        }
        ClaudeContent::Blocks(blocks) => {
            let texts: Vec<&str> = blocks
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            if texts.is_empty() {
                None
            } else {
                Some(texts.join(""))
            }
        }
    }
}

/// Map a Claude role string to the canonical ABP role string.
///
/// Claude uses `"user"` and `"assistant"`; this function normalises them
/// for consistency.
#[must_use]
pub fn map_role_to_abp(claude_role: &str) -> &'static str {
    match claude_role {
        "user" => "user",
        "assistant" => "assistant",
        _ => "user",
    }
}

/// Map an ABP role string back to a Claude role string.
///
/// Returns `"user"` or `"assistant"`.
#[must_use]
pub fn map_role_from_abp(abp_role: &str) -> &'static str {
    match abp_role {
        "user" => "user",
        "assistant" => "assistant",
        "system" => "user",
        "tool" => "user",
        _ => "user",
    }
}

/// Convert a list of [`ClaudeTool`] definitions into a JSON-serializable
/// representation suitable for storing in the work order's vendor map.
#[must_use]
pub fn tools_to_vendor_json(tools: &[ClaudeTool]) -> serde_json::Value {
    serde_json::to_value(tools).unwrap_or_default()
}

/// Extract [`ClaudeUsage`] from a raw JSON value.
///
/// Expects the Claude-style `{ "input_tokens": N, "output_tokens": M }` shape.
/// Returns zero-valued usage if parsing fails.
#[must_use]
pub fn usage_from_raw(raw: &serde_json::Value) -> ClaudeUsage {
    let input_tokens = raw
        .get("input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output_tokens = raw
        .get("output_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let cache_creation = raw
        .get("cache_creation_input_tokens")
        .and_then(|v| v.as_u64());
    let cache_read = raw.get("cache_read_input_tokens").and_then(|v| v.as_u64());

    ClaudeUsage {
        input_tokens,
        output_tokens,
        cache_creation_input_tokens: cache_creation,
        cache_read_input_tokens: cache_read,
    }
}

/// Convert a shim [`ContentBlock`] to a sequence of [`AgentEventKind`] values.
///
/// This is useful when ingesting Claude-shaped content blocks into the ABP
/// event trace.
#[must_use]
pub fn content_block_to_event_kind(block: &ContentBlock) -> Option<AgentEventKind> {
    match block {
        ContentBlock::Text { text } => {
            Some(AgentEventKind::AssistantMessage { text: text.clone() })
        }
        ContentBlock::ToolUse { id, name, input } => Some(AgentEventKind::ToolCall {
            tool_name: name.clone(),
            tool_use_id: Some(id.clone()),
            parent_tool_use_id: None,
            input: input.clone(),
        }),
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => Some(AgentEventKind::ToolResult {
            tool_name: String::new(),
            tool_use_id: Some(tool_use_id.clone()),
            output: json!(content),
            is_error: is_error.unwrap_or(false),
        }),
        ContentBlock::Thinking { thinking, .. } => Some(AgentEventKind::AssistantMessage {
            text: thinking.clone(),
        }),
        ContentBlock::Image { .. } => None,
    }
}

/// Build a [`MessagesResponse`] from a set of content blocks and metadata.
///
/// Convenience wrapper for constructing well-formed responses.
#[must_use]
pub fn build_response(
    id: &str,
    model: &str,
    content: Vec<ContentBlock>,
    stop_reason: Option<String>,
    usage: ClaudeUsage,
) -> MessagesResponse {
    MessagesResponse {
        id: id.to_string(),
        type_field: "message".to_string(),
        role: "assistant".to_string(),
        content,
        model: model.to_string(),
        stop_reason,
        usage,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ClaudeMessage, ClaudeToolChoice, ImageSource};
    use abp_core::{AgentEvent, AgentEventKind, Outcome, ReceiptBuilder};
    use chrono::Utc;
    use serde_json::json;

    // -- Helpers ----------------------------------------------------------

    fn simple_request(text: &str) -> MessagesRequest {
        MessagesRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            messages: vec![ClaudeMessage {
                role: "user".to_string(),
                content: ClaudeContent::Text(text.to_string()),
            }],
            max_tokens: 4096,
            system: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stream: None,
            tools: None,
            tool_choice: None,

            stop_sequences: None,

            thinking: None,
        }
    }

    fn make_receipt(events: Vec<AgentEvent>, usage_raw: serde_json::Value) -> Receipt {
        let mut builder = ReceiptBuilder::new("claude-shim")
            .outcome(Outcome::Complete)
            .usage_raw(usage_raw);
        for e in events {
            builder = builder.add_trace_event(e);
        }
        builder.build()
    }

    fn text_event(text: &str) -> AgentEvent {
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
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
                message: "done".to_string(),
            },
            ext: None,
        }
    }

    // ── 1. to_work_order basic ──────────────────────────────────────────

    #[test]
    fn to_work_order_extracts_model() {
        let req = simple_request("hello");
        let wo = to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("claude-sonnet-4-20250514"));
    }

    #[test]
    fn to_work_order_extracts_task_from_user_message() {
        let req = simple_request("Refactor the auth module");
        let wo = to_work_order(&req);
        assert_eq!(wo.task, "Refactor the auth module");
    }

    #[test]
    fn to_work_order_stores_dialect() {
        let req = simple_request("hi");
        let wo = to_work_order(&req);
        let dialect = wo.config.vendor.get("dialect").unwrap();
        assert_eq!(dialect, &json!("claude"));
    }

    #[test]
    fn to_work_order_stores_max_tokens() {
        let req = simple_request("hi");
        let wo = to_work_order(&req);
        let max_tokens = wo.config.vendor.get("max_tokens").unwrap();
        assert_eq!(max_tokens, &json!(4096));
    }

    // ── 2. System message extraction ────────────────────────────────────

    #[test]
    fn to_work_order_stores_system_prompt() {
        let mut req = simple_request("hello");
        req.system = Some("You are a helpful assistant.".to_string());
        let wo = to_work_order(&req);
        let system = wo.config.vendor.get("system").unwrap();
        assert_eq!(system, &json!("You are a helpful assistant."));
    }

    #[test]
    fn to_work_order_system_fallback_for_task() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            messages: vec![],
            max_tokens: 1024,
            system: Some("Be helpful".to_string()),
            temperature: None,
            top_p: None,
            top_k: None,
            stream: None,
            tools: None,
            tool_choice: None,

            stop_sequences: None,

            thinking: None,
        };
        let wo = to_work_order(&req);
        assert_eq!(wo.task, "Be helpful");
    }

    #[test]
    fn to_work_order_default_task_no_messages_no_system() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            messages: vec![],
            max_tokens: 1024,
            system: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stream: None,
            tools: None,
            tool_choice: None,

            stop_sequences: None,

            thinking: None,
        };
        let wo = to_work_order(&req);
        assert_eq!(wo.task, "Claude shim request");
    }

    // ── 3. Temperature / top_p / top_k / stream ────────────────────────

    #[test]
    fn to_work_order_stores_temperature() {
        let mut req = simple_request("hi");
        req.temperature = Some(0.7);
        let wo = to_work_order(&req);
        let temp = wo.config.vendor.get("temperature").unwrap();
        assert_eq!(temp, &json!(0.7));
    }

    #[test]
    fn to_work_order_stores_top_p() {
        let mut req = simple_request("hi");
        req.top_p = Some(0.9);
        let wo = to_work_order(&req);
        let top_p = wo.config.vendor.get("top_p").unwrap();
        assert_eq!(top_p, &json!(0.9));
    }

    #[test]
    fn to_work_order_stores_top_k() {
        let mut req = simple_request("hi");
        req.top_k = Some(40);
        let wo = to_work_order(&req);
        let top_k = wo.config.vendor.get("top_k").unwrap();
        assert_eq!(top_k, &json!(40));
    }

    #[test]
    fn to_work_order_stores_stream() {
        let mut req = simple_request("hi");
        req.stream = Some(true);
        let wo = to_work_order(&req);
        let stream = wo.config.vendor.get("stream").unwrap();
        assert_eq!(stream, &json!(true));
    }

    #[test]
    fn to_work_order_omits_none_fields() {
        let req = simple_request("hi");
        let wo = to_work_order(&req);
        assert!(!wo.config.vendor.contains_key("temperature"));
        assert!(!wo.config.vendor.contains_key("top_p"));
        assert!(!wo.config.vendor.contains_key("top_k"));
        assert!(!wo.config.vendor.contains_key("stream"));
        assert!(!wo.config.vendor.contains_key("system"));
    }

    // ── 4. Tools mapping ────────────────────────────────────────────────

    #[test]
    fn to_work_order_stores_tools() {
        let mut req = simple_request("hi");
        req.tools = Some(vec![ClaudeTool {
            name: "read_file".to_string(),
            description: Some("Read a file".to_string()),
            input_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        }]);
        let wo = to_work_order(&req);
        let tools = wo.config.vendor.get("tools").unwrap();
        assert!(tools.is_array());
        assert_eq!(tools.as_array().unwrap().len(), 1);
    }

    #[test]
    fn to_work_order_stores_tool_choice() {
        let mut req = simple_request("hi");
        req.tool_choice = Some(ClaudeToolChoice::Auto {});
        let wo = to_work_order(&req);
        assert!(wo.config.vendor.contains_key("tool_choice"));
    }

    // ── 5. Content block types ──────────────────────────────────────────

    #[test]
    fn to_work_order_blocks_content() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            messages: vec![ClaudeMessage {
                role: "user".to_string(),
                content: ClaudeContent::Blocks(vec![
                    ContentBlock::Text {
                        text: "Look at this:".to_string(),
                    },
                    ContentBlock::Image {
                        source: ImageSource::Url {
                            url: "https://example.com/img.png".to_string(),
                        },
                    },
                ]),
            }],
            max_tokens: 1024,
            system: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stream: None,
            tools: None,
            tool_choice: None,

            stop_sequences: None,

            thinking: None,
        };
        let wo = to_work_order(&req);
        assert_eq!(wo.task, "Look at this:");
    }

    #[test]
    fn to_work_order_tool_result_in_content() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            messages: vec![
                ClaudeMessage {
                    role: "user".to_string(),
                    content: ClaudeContent::Text("call a tool".to_string()),
                },
                ClaudeMessage {
                    role: "user".to_string(),
                    content: ClaudeContent::Blocks(vec![ContentBlock::ToolResult {
                        tool_use_id: "tu_1".to_string(),
                        content: "file contents here".to_string(),
                        is_error: None,
                    }]),
                },
            ],
            max_tokens: 1024,
            system: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stream: None,
            tools: None,
            tool_choice: None,

            stop_sequences: None,

            thinking: None,
        };
        let wo = to_work_order(&req);
        // Task should fallback since last user message has only ToolResult (no text)
        assert_eq!(wo.task, "call a tool");
    }

    // ── 6. from_receipt basic ───────────────────────────────────────────

    #[test]
    fn from_receipt_text_response() {
        let req = simple_request("hi");
        let wo = to_work_order(&req);
        let receipt = make_receipt(
            vec![text_event("Hello there!")],
            json!({"input_tokens": 10, "output_tokens": 20}),
        );
        let resp = from_receipt(&receipt, &wo);
        assert_eq!(resp.type_field, "message");
        assert_eq!(resp.role, "assistant");
        assert_eq!(resp.content.len(), 1);
        assert!(matches!(
            &resp.content[0],
            ContentBlock::Text { text } if text == "Hello there!"
        ));
    }

    #[test]
    fn from_receipt_stop_reason_end_turn() {
        let req = simple_request("hi");
        let wo = to_work_order(&req);
        let receipt = make_receipt(vec![text_event("done"), run_completed_event()], json!({}));
        let resp = from_receipt(&receipt, &wo);
        assert_eq!(resp.stop_reason.as_deref(), Some("end_turn"));
    }

    #[test]
    fn from_receipt_stop_reason_tool_use() {
        let req = simple_request("hi");
        let wo = to_work_order(&req);
        let receipt = make_receipt(
            vec![tool_call_event(
                "read_file",
                "tu_1",
                json!({"path": "a.rs"}),
            )],
            json!({}),
        );
        let resp = from_receipt(&receipt, &wo);
        assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
    }

    #[test]
    fn from_receipt_usage_extracted() {
        let req = simple_request("hi");
        let wo = to_work_order(&req);
        let receipt = make_receipt(
            vec![text_event("ok")],
            json!({"input_tokens": 42, "output_tokens": 17}),
        );
        let resp = from_receipt(&receipt, &wo);
        assert_eq!(resp.usage.input_tokens, 42);
        assert_eq!(resp.usage.output_tokens, 17);
    }

    #[test]
    fn from_receipt_model_from_work_order() {
        let mut req = simple_request("hi");
        req.model = "claude-opus-4-20250514".to_string();
        let wo = to_work_order(&req);
        let receipt = make_receipt(vec![text_event("ok")], json!({}));
        let resp = from_receipt(&receipt, &wo);
        assert_eq!(resp.model, "claude-opus-4-20250514");
    }

    #[test]
    fn from_receipt_empty_trace() {
        let req = simple_request("hi");
        let wo = to_work_order(&req);
        let receipt = make_receipt(vec![], json!({}));
        let resp = from_receipt(&receipt, &wo);
        assert!(resp.content.is_empty());
        assert!(resp.stop_reason.is_none());
    }

    // ── 7. from_receipt with tool calls ─────────────────────────────────

    #[test]
    fn from_receipt_tool_call_content_block() {
        let req = simple_request("hi");
        let wo = to_work_order(&req);
        let receipt = make_receipt(
            vec![tool_call_event("bash", "tu_42", json!({"command": "ls"}))],
            json!({}),
        );
        let resp = from_receipt(&receipt, &wo);
        assert!(matches!(
            &resp.content[0],
            ContentBlock::ToolUse { id, name, .. } if id == "tu_42" && name == "bash"
        ));
    }

    #[test]
    fn from_receipt_mixed_text_and_tool() {
        let req = simple_request("hi");
        let wo = to_work_order(&req);
        let receipt = make_receipt(
            vec![
                text_event("Let me check"),
                tool_call_event("read_file", "tu_1", json!({"path": "x"})),
            ],
            json!({}),
        );
        let resp = from_receipt(&receipt, &wo);
        assert_eq!(resp.content.len(), 2);
        assert!(matches!(&resp.content[0], ContentBlock::Text { .. }));
        assert!(matches!(&resp.content[1], ContentBlock::ToolUse { .. }));
        assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
    }

    // ── 8. from_agent_event streaming ───────────────────────────────────

    #[test]
    fn from_agent_event_text_delta() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "Hello".to_string(),
            },
            ext: None,
        };
        let se = from_agent_event(&event).unwrap();
        assert!(matches!(
            se,
            StreamEvent::ContentBlockDelta {
                delta: StreamDelta::TextDelta { ref text },
                ..
            } if text == "Hello"
        ));
    }

    #[test]
    fn from_agent_event_tool_call() {
        let event = tool_call_event("write_file", "tu_99", json!({"path": "b.rs"}));
        let se = from_agent_event(&event).unwrap();
        assert!(matches!(se, StreamEvent::ContentBlockStart { .. }));
    }

    #[test]
    fn from_agent_event_run_completed() {
        let event = run_completed_event();
        let se = from_agent_event(&event).unwrap();
        assert!(matches!(
            se,
            StreamEvent::MessageDelta {
                delta: MessageDeltaBody {
                    stop_reason: Some(ref reason),
                    ..
                },
                ..
            } if reason == "end_turn"
        ));
    }

    #[test]
    fn from_agent_event_assistant_message() {
        let event = text_event("full message");
        let se = from_agent_event(&event).unwrap();
        assert!(matches!(
            se,
            StreamEvent::ContentBlockDelta {
                delta: StreamDelta::TextDelta { ref text },
                ..
            } if text == "full message"
        ));
    }

    #[test]
    fn from_agent_event_returns_none_for_warning() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Warning {
                message: "watch out".to_string(),
            },
            ext: None,
        };
        assert!(from_agent_event(&event).is_none());
    }

    #[test]
    fn from_agent_event_returns_none_for_file_changed() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::FileChanged {
                path: "src/main.rs".to_string(),
                summary: "modified".to_string(),
            },
            ext: None,
        };
        assert!(from_agent_event(&event).is_none());
    }

    #[test]
    fn from_agent_event_returns_none_for_run_started() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "starting".to_string(),
            },
            ext: None,
        };
        assert!(from_agent_event(&event).is_none());
    }

    // ── 9. Helper functions ─────────────────────────────────────────────

    #[test]
    fn content_to_text_plain_string() {
        let c = ClaudeContent::Text("hello".to_string());
        assert_eq!(content_to_text(&c), Some("hello".to_string()));
    }

    #[test]
    fn content_to_text_empty_string() {
        let c = ClaudeContent::Text(String::new());
        assert!(content_to_text(&c).is_none());
    }

    #[test]
    fn content_to_text_blocks() {
        let c = ClaudeContent::Blocks(vec![
            ContentBlock::Text {
                text: "a".to_string(),
            },
            ContentBlock::Text {
                text: "b".to_string(),
            },
        ]);
        assert_eq!(content_to_text(&c), Some("ab".to_string()));
    }

    #[test]
    fn content_to_text_blocks_no_text() {
        let c = ClaudeContent::Blocks(vec![ContentBlock::ToolResult {
            tool_use_id: "tu_1".to_string(),
            content: "result".to_string(),
            is_error: None,
        }]);
        assert!(content_to_text(&c).is_none());
    }

    #[test]
    fn map_role_to_abp_user() {
        assert_eq!(map_role_to_abp("user"), "user");
    }

    #[test]
    fn map_role_to_abp_assistant() {
        assert_eq!(map_role_to_abp("assistant"), "assistant");
    }

    #[test]
    fn map_role_to_abp_unknown() {
        assert_eq!(map_role_to_abp("other"), "user");
    }

    #[test]
    fn map_role_from_abp_system() {
        assert_eq!(map_role_from_abp("system"), "user");
    }

    #[test]
    fn map_role_from_abp_tool() {
        assert_eq!(map_role_from_abp("tool"), "user");
    }

    // ── 10. usage_from_raw ──────────────────────────────────────────────

    #[test]
    fn usage_from_raw_full() {
        let raw = json!({
            "input_tokens": 100,
            "output_tokens": 50,
            "cache_creation_input_tokens": 10,
            "cache_read_input_tokens": 5
        });
        let u = usage_from_raw(&raw);
        assert_eq!(u.input_tokens, 100);
        assert_eq!(u.output_tokens, 50);
        assert_eq!(u.cache_creation_input_tokens, Some(10));
        assert_eq!(u.cache_read_input_tokens, Some(5));
    }

    #[test]
    fn usage_from_raw_empty() {
        let u = usage_from_raw(&json!({}));
        assert_eq!(u.input_tokens, 0);
        assert_eq!(u.output_tokens, 0);
        assert!(u.cache_creation_input_tokens.is_none());
        assert!(u.cache_read_input_tokens.is_none());
    }

    #[test]
    fn usage_from_raw_partial() {
        let u = usage_from_raw(&json!({"input_tokens": 7}));
        assert_eq!(u.input_tokens, 7);
        assert_eq!(u.output_tokens, 0);
    }

    // ── 11. content_block_to_event_kind ─────────────────────────────────

    #[test]
    fn content_block_to_event_text() {
        let block = ContentBlock::Text {
            text: "hello".to_string(),
        };
        let kind = content_block_to_event_kind(&block).unwrap();
        assert!(matches!(kind, AgentEventKind::AssistantMessage { text } if text == "hello"));
    }

    #[test]
    fn content_block_to_event_tool_use() {
        let block = ContentBlock::ToolUse {
            id: "tu_1".to_string(),
            name: "bash".to_string(),
            input: json!({"cmd": "ls"}),
        };
        let kind = content_block_to_event_kind(&block).unwrap();
        assert!(matches!(kind, AgentEventKind::ToolCall { tool_name, .. } if tool_name == "bash"));
    }

    #[test]
    fn content_block_to_event_tool_result() {
        let block = ContentBlock::ToolResult {
            tool_use_id: "tu_1".to_string(),
            content: "output".to_string(),
            is_error: None,
        };
        let kind = content_block_to_event_kind(&block).unwrap();
        assert!(matches!(kind, AgentEventKind::ToolResult { .. }));
    }

    #[test]
    fn content_block_to_event_image_none() {
        let block = ContentBlock::Image {
            source: ImageSource::Url {
                url: "https://example.com/img.png".to_string(),
            },
        };
        assert!(content_block_to_event_kind(&block).is_none());
    }

    // ── 12. build_response helper ───────────────────────────────────────

    #[test]
    fn build_response_sets_type_and_role() {
        let resp = build_response(
            "msg_1",
            "claude-sonnet-4-20250514",
            vec![],
            None,
            ClaudeUsage {
                input_tokens: 0,
                output_tokens: 0,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
        );
        assert_eq!(resp.type_field, "message");
        assert_eq!(resp.role, "assistant");
        assert_eq!(resp.id, "msg_1");
    }

    // ── 13. Roundtrip: request → work_order → receipt → response ───────

    #[test]
    fn roundtrip_simple_text() {
        let req = simple_request("Tell me a joke");
        let wo = to_work_order(&req);

        let receipt = make_receipt(
            vec![
                text_event("Why did the chicken cross the road?"),
                run_completed_event(),
            ],
            json!({"input_tokens": 5, "output_tokens": 10}),
        );

        let resp = from_receipt(&receipt, &wo);
        assert_eq!(resp.model, "claude-sonnet-4-20250514");
        assert_eq!(resp.stop_reason.as_deref(), Some("end_turn"));
        assert_eq!(resp.usage.input_tokens, 5);
        assert_eq!(resp.usage.output_tokens, 10);
        assert_eq!(resp.content.len(), 1);
    }

    #[test]
    fn roundtrip_with_tools() {
        let mut req = simple_request("Read main.rs");
        req.tools = Some(vec![ClaudeTool {
            name: "read_file".to_string(),
            description: Some("Read a file".to_string()),
            input_schema: json!({"type": "object"}),
        }]);

        let wo = to_work_order(&req);
        assert!(wo.config.vendor.contains_key("tools"));

        let receipt = make_receipt(
            vec![tool_call_event(
                "read_file",
                "tu_1",
                json!({"path": "main.rs"}),
            )],
            json!({"input_tokens": 8, "output_tokens": 3}),
        );

        let resp = from_receipt(&receipt, &wo);
        assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
        assert!(
            matches!(&resp.content[0], ContentBlock::ToolUse { name, .. } if name == "read_file")
        );
    }

    #[test]
    fn roundtrip_with_system_prompt() {
        let mut req = simple_request("hello");
        req.system = Some("You are a pirate.".to_string());

        let wo = to_work_order(&req);
        assert_eq!(
            wo.config.vendor.get("system").unwrap(),
            &json!("You are a pirate.")
        );

        let receipt = make_receipt(vec![text_event("Ahoy!")], json!({}));
        let resp = from_receipt(&receipt, &wo);
        assert_eq!(resp.content.len(), 1);
    }

    // ── 14. tools_to_vendor_json ────────────────────────────────────────

    #[test]
    fn tools_to_vendor_json_roundtrip() {
        let tools = vec![
            ClaudeTool {
                name: "a".to_string(),
                description: Some("tool a".to_string()),
                input_schema: json!({}),
            },
            ClaudeTool {
                name: "b".to_string(),
                description: None,
                input_schema: json!({"type": "object"}),
            },
        ];
        let v = tools_to_vendor_json(&tools);
        let back: Vec<ClaudeTool> = serde_json::from_value(v).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].name, "a");
        assert_eq!(back[1].name, "b");
    }

    // ── 15. Messages stored in vendor map ───────────────────────────────

    #[test]
    fn to_work_order_stores_messages() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            messages: vec![
                ClaudeMessage {
                    role: "user".to_string(),
                    content: ClaudeContent::Text("first".to_string()),
                },
                ClaudeMessage {
                    role: "assistant".to_string(),
                    content: ClaudeContent::Text("reply".to_string()),
                },
                ClaudeMessage {
                    role: "user".to_string(),
                    content: ClaudeContent::Text("second".to_string()),
                },
            ],
            max_tokens: 1024,
            system: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stream: None,
            tools: None,
            tool_choice: None,

            stop_sequences: None,

            thinking: None,
        };
        let wo = to_work_order(&req);
        let msgs = wo.config.vendor.get("messages").unwrap();
        assert!(msgs.is_array());
        assert_eq!(msgs.as_array().unwrap().len(), 3);
        // Task comes from last user message
        assert_eq!(wo.task, "second");
    }

    // ── 16. Edge: assistant message as last ─────────────────────────────

    #[test]
    fn extract_task_assistant_last_uses_previous_user() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            messages: vec![
                ClaudeMessage {
                    role: "user".to_string(),
                    content: ClaudeContent::Text("my task".to_string()),
                },
                ClaudeMessage {
                    role: "assistant".to_string(),
                    content: ClaudeContent::Text("sure".to_string()),
                },
            ],
            max_tokens: 1024,
            system: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stream: None,
            tools: None,
            tool_choice: None,

            stop_sequences: None,

            thinking: None,
        };
        assert_eq!(extract_task(&req), "my task");
    }

    // ── 17. from_receipt with cache usage ───────────────────────────────

    #[test]
    fn from_receipt_cache_usage() {
        let req = simple_request("hi");
        let wo = to_work_order(&req);
        let receipt = make_receipt(
            vec![text_event("ok")],
            json!({
                "input_tokens": 100,
                "output_tokens": 50,
                "cache_creation_input_tokens": 20,
                "cache_read_input_tokens": 30
            }),
        );
        let resp = from_receipt(&receipt, &wo);
        assert_eq!(resp.usage.cache_creation_input_tokens, Some(20));
        assert_eq!(resp.usage.cache_read_input_tokens, Some(30));
    }

    // ── 18. Multiple text events → multiple content blocks ──────────────

    #[test]
    fn from_receipt_multiple_text_blocks() {
        let req = simple_request("hi");
        let wo = to_work_order(&req);
        let receipt = make_receipt(vec![text_event("first"), text_event("second")], json!({}));
        let resp = from_receipt(&receipt, &wo);
        assert_eq!(resp.content.len(), 2);
    }

    // ── 19. Thinking block support ──────────────────────────────────────

    fn thinking_event(text: &str) -> AgentEvent {
        let mut ext = std::collections::BTreeMap::new();
        ext.insert("thinking".into(), serde_json::Value::Bool(true));
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: text.to_string(),
            },
            ext: Some(ext),
        }
    }

    fn thinking_event_with_sig(text: &str, sig: &str) -> AgentEvent {
        let mut ext = std::collections::BTreeMap::new();
        ext.insert("thinking".into(), serde_json::Value::Bool(true));
        ext.insert("signature".into(), serde_json::Value::String(sig.into()));
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: text.to_string(),
            },
            ext: Some(ext),
        }
    }

    #[test]
    fn from_receipt_thinking_block() {
        let req = simple_request("hi");
        let wo = to_work_order(&req);
        let receipt = make_receipt(
            vec![
                thinking_event("Let me reason..."),
                text_event("The answer is 42."),
            ],
            json!({}),
        );
        let resp = from_receipt(&receipt, &wo);
        assert_eq!(resp.content.len(), 2);
        assert!(matches!(
            &resp.content[0],
            ContentBlock::Thinking { thinking, .. } if thinking == "Let me reason..."
        ));
        assert!(matches!(
            &resp.content[1],
            ContentBlock::Text { text } if text == "The answer is 42."
        ));
    }

    #[test]
    fn from_receipt_thinking_with_signature() {
        let req = simple_request("hi");
        let wo = to_work_order(&req);
        let receipt = make_receipt(
            vec![thinking_event_with_sig("step by step", "sig_abc")],
            json!({}),
        );
        let resp = from_receipt(&receipt, &wo);
        assert!(matches!(
            &resp.content[0],
            ContentBlock::Thinking { thinking, signature } if thinking == "step by step" && signature.as_deref() == Some("sig_abc")
        ));
    }

    // ── 20. Thinking in streaming ───────────────────────────────────────

    #[test]
    fn from_agent_event_thinking_produces_thinking_delta() {
        let event = thinking_event("considering...");
        let se = from_agent_event(&event).unwrap();
        assert!(matches!(
            se,
            StreamEvent::ContentBlockDelta {
                delta: StreamDelta::ThinkingDelta { ref thinking },
                ..
            } if thinking == "considering..."
        ));
    }

    // ── 21. Error event mapping ─────────────────────────────────────────

    #[test]
    fn from_agent_event_error_produces_error_stream_event() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error {
                message: "rate limit exceeded".to_string(),
                error_code: None,
            },
            ext: None,
        };
        let se = from_agent_event(&event).unwrap();
        match se {
            StreamEvent::Error { error } => {
                assert_eq!(error.error_type, "api_error");
                assert_eq!(error.message, "rate limit exceeded");
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    // ── 22. Thinking content block ↔ event kind ─────────────────────────

    #[test]
    fn content_block_to_event_thinking() {
        let block = ContentBlock::Thinking {
            thinking: "deep thought".to_string(),
            signature: Some("sig_xyz".to_string()),
        };
        let kind = content_block_to_event_kind(&block).unwrap();
        assert!(
            matches!(kind, AgentEventKind::AssistantMessage { text } if text == "deep thought")
        );
    }

    // ── 23. ToolResult is_error propagation ─────────────────────────────

    #[test]
    fn content_block_to_event_tool_result_is_error() {
        let block = ContentBlock::ToolResult {
            tool_use_id: "tu_1".to_string(),
            content: "permission denied".to_string(),
            is_error: Some(true),
        };
        let kind = content_block_to_event_kind(&block).unwrap();
        match kind {
            AgentEventKind::ToolResult { is_error, .. } => assert!(is_error),
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn content_block_to_event_tool_result_no_error() {
        let block = ContentBlock::ToolResult {
            tool_use_id: "tu_1".to_string(),
            content: "success".to_string(),
            is_error: None,
        };
        let kind = content_block_to_event_kind(&block).unwrap();
        match kind {
            AgentEventKind::ToolResult { is_error, .. } => assert!(!is_error),
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    // ── 24. Thinking config in work order ───────────────────────────────

    #[test]
    fn to_work_order_stores_thinking() {
        let mut req = simple_request("hi");
        req.thinking = Some(crate::types::ThinkingConfig::new(2048));
        let wo = to_work_order(&req);
        let thinking = wo.config.vendor.get("thinking").unwrap();
        assert_eq!(thinking.get("budget_tokens").unwrap().as_u64(), Some(2048));
    }

    // ── 25. Stop sequences in work order ────────────────────────────────

    #[test]
    fn to_work_order_stores_stop_sequences() {
        let mut req = simple_request("hi");
        req.stop_sequences = Some(vec!["STOP".to_string(), "END".to_string()]);
        let wo = to_work_order(&req);
        let stops = wo.config.vendor.get("stop_sequences").unwrap();
        assert!(stops.is_array());
        assert_eq!(stops.as_array().unwrap().len(), 2);
    }

    // ── 26. Thinking block serde roundtrip ──────────────────────────────

    #[test]
    fn thinking_content_block_serde_roundtrip() {
        let block = ContentBlock::Thinking {
            thinking: "Let me consider...".to_string(),
            signature: Some("sig_abc".to_string()),
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains(r#""type":"thinking""#));
        let back: ContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(block, back);
    }

    #[test]
    fn thinking_content_block_no_signature_roundtrip() {
        let block = ContentBlock::Thinking {
            thinking: "reasoning text".to_string(),
            signature: None,
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(!json.contains("signature"));
        let back: ContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(block, back);
    }

    // ── 27. StreamDelta ThinkingDelta serde ─────────────────────────────

    #[test]
    fn stream_delta_thinking_serde_roundtrip() {
        let delta = StreamDelta::ThinkingDelta {
            thinking: "partial thought".to_string(),
        };
        let json = serde_json::to_string(&delta).unwrap();
        assert!(json.contains(r#""type":"thinking_delta""#));
        let back: StreamDelta = serde_json::from_str(&json).unwrap();
        assert_eq!(delta, back);
    }

    #[test]
    fn stream_delta_signature_serde_roundtrip() {
        let delta = StreamDelta::SignatureDelta {
            signature: "partial_sig".to_string(),
        };
        let json = serde_json::to_string(&delta).unwrap();
        assert!(json.contains(r#""type":"signature_delta""#));
        let back: StreamDelta = serde_json::from_str(&json).unwrap();
        assert_eq!(delta, back);
    }

    // ── 28. Error stream event serde ────────────────────────────────────

    #[test]
    fn error_stream_event_serde_roundtrip() {
        let event = StreamEvent::Error {
            error: crate::types::ErrorResponse {
                error_type: "overloaded_error".to_string(),
                message: "Server is overloaded".to_string(),
            },
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"error""#));
        let back: StreamEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, back);
    }

    // ── 29. ErrorResponse parsing ───────────────────────────────────────

    #[test]
    fn error_response_from_json() {
        let json = r#"{"type":"invalid_request_error","message":"max_tokens: must be > 0"}"#;
        let err: crate::types::ErrorResponse = serde_json::from_str(json).unwrap();
        assert_eq!(err.error_type, "invalid_request_error");
        assert_eq!(err.message, "max_tokens: must be > 0");
    }

    #[test]
    fn error_response_serde_roundtrip() {
        let err = crate::types::ErrorResponse {
            error_type: "authentication_error".to_string(),
            message: "Invalid API key".to_string(),
        };
        let json = serde_json::to_string(&err).unwrap();
        let back: crate::types::ErrorResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(err, back);
    }

    // ── 30. ThinkingConfig serde ────────────────────────────────────────

    #[test]
    fn thinking_config_serde_roundtrip() {
        let cfg = crate::types::ThinkingConfig::new(4096);
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(json.contains(r#""type":"enabled""#));
        assert!(json.contains("4096"));
        let back: crate::types::ThinkingConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg, back);
    }

    // ── 31. ToolResult is_error serde roundtrip ─────────────────────────

    #[test]
    fn tool_result_is_error_true_serde() {
        let block = ContentBlock::ToolResult {
            tool_use_id: "tu_1".to_string(),
            content: "error output".to_string(),
            is_error: Some(true),
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains(r#""is_error":true"#));
        let back: ContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(block, back);
    }

    #[test]
    fn tool_result_is_error_omitted_when_none() {
        let block = ContentBlock::ToolResult {
            tool_use_id: "tu_1".to_string(),
            content: "output".to_string(),
            is_error: None,
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(!json.contains("is_error"));
    }

    // ── 32. MessagesRequest new fields serde ────────────────────────────

    #[test]
    fn request_with_thinking_serde_roundtrip() {
        let mut req = simple_request("think about this");
        req.thinking = Some(crate::types::ThinkingConfig::new(8192));
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("thinking"));
        assert!(json.contains("8192"));
        let back: MessagesRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.thinking.as_ref().unwrap().budget_tokens, 8192);
    }

    #[test]
    fn request_with_stop_sequences_serde_roundtrip() {
        let mut req = simple_request("stop me");
        req.stop_sequences = Some(vec!["STOP".into(), "###".into()]);
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("stop_sequences"));
        let back: MessagesRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.stop_sequences.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn request_omits_new_none_fields() {
        let req = simple_request("hi");
        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.contains("thinking"));
        assert!(!json.contains("stop_sequences"));
    }

    // ── 33. Mixed thinking + tools in receipt ───────────────────────────

    #[test]
    fn from_receipt_thinking_then_tool_call() {
        let req = simple_request("hi");
        let wo = to_work_order(&req);
        let receipt = make_receipt(
            vec![
                thinking_event("I should read the file first"),
                tool_call_event("read_file", "tu_1", json!({"path": "main.rs"})),
            ],
            json!({}),
        );
        let resp = from_receipt(&receipt, &wo);
        assert_eq!(resp.content.len(), 2);
        assert!(matches!(&resp.content[0], ContentBlock::Thinking { .. }));
        assert!(matches!(&resp.content[1], ContentBlock::ToolUse { .. }));
        assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
    }
}
