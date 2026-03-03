// SPDX-License-Identifier: MIT OR Apache-2.0
//! Anthropic Messages API surface types and conversions to/from ABP core.
//!
//! This module provides types that mirror the
//! [Anthropic Messages API](https://docs.anthropic.com/en/api/messages)
//! and implements bidirectional conversion with ABP's [`WorkOrder`] and
//! [`Receipt`] types.

use std::collections::BTreeMap;

use abp_core::{AgentEventKind, Outcome, Receipt, RuntimeConfig, WorkOrder, WorkOrderBuilder};
use serde::{Deserialize, Serialize};

// Re-export dialect types under API-matching names.
pub use crate::dialect::{
    ClaudeCacheControl as CacheControl, ClaudeContentBlock as ContentBlock,
    ClaudeImageSource as ImageSource, ClaudeMessageDelta as MessageDelta,
    ClaudeStreamDelta as StreamDelta, ClaudeStreamEvent as StreamEvent,
    ClaudeSystemBlock as SystemBlock, ClaudeToolDef as Tool, ClaudeUsage as Usage,
};

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

/// Role of a message participant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// The human user.
    User,
    /// The AI assistant.
    Assistant,
}

/// Message content — either a plain text string or structured content blocks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    /// Array of structured content blocks.
    Blocks(Vec<ContentBlock>),
    /// Plain text string content.
    Text(String),
}

/// A single message in a conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    /// Message role.
    pub role: Role,
    /// Message content (string or content blocks).
    pub content: MessageContent,
}

/// System prompt — either a plain string or structured blocks with cache control.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SystemMessage {
    /// Array of structured system blocks (with optional cache control).
    Blocks(Vec<SystemBlock>),
    /// Plain text system prompt.
    Text(String),
}

/// Request metadata sent to the Anthropic API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metadata {
    /// An external identifier for the user making the request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
}

/// Anthropic Messages API request.
///
/// Mirrors the JSON body of `POST /v1/messages`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MessagesRequest {
    /// Model identifier (e.g. `claude-sonnet-4-20250514`).
    pub model: String,
    /// Conversation messages.
    pub messages: Vec<Message>,
    /// Maximum number of tokens to generate.
    pub max_tokens: u32,
    /// System prompt (string or structured blocks).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<SystemMessage>,
    /// Tools available to the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    /// Request metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Metadata>,
    /// Whether to stream the response via SSE.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    /// Custom stop sequences.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    /// Sampling temperature (0.0–1.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Nucleus sampling parameter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    /// Top-k sampling parameter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// Anthropic Messages API response.
///
/// Mirrors the JSON body returned by `POST /v1/messages`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MessagesResponse {
    /// Unique message identifier (e.g. `msg_01XFDUDYJgAACzvnptvVoYEL`).
    pub id: String,
    /// Object type — always `"message"`.
    #[serde(rename = "type")]
    pub response_type: String,
    /// Role — always `"assistant"`.
    pub role: String,
    /// Content blocks in the response.
    pub content: Vec<ContentBlock>,
    /// Model that generated the response.
    pub model: String,
    /// Reason the model stopped generating.
    pub stop_reason: Option<String>,
    /// The stop sequence that triggered the stop, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,
    /// Token usage statistics.
    pub usage: Usage,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract the text from a [`SystemMessage`].
fn system_message_text(sys: &SystemMessage) -> String {
    match sys {
        SystemMessage::Text(s) => s.clone(),
        SystemMessage::Blocks(blocks) => blocks
            .iter()
            .map(|b| match b {
                SystemBlock::Text { text, .. } => text.as_str(),
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

/// Extract the plain-text task from a sequence of messages.
fn extract_task(messages: &[Message]) -> String {
    messages
        .iter()
        .filter(|m| m.role == Role::User)
        .map(|m| match &m.content {
            MessageContent::Text(s) => s.clone(),
            MessageContent::Blocks(blocks) => blocks
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

// ---------------------------------------------------------------------------
// Conversions
// ---------------------------------------------------------------------------

impl From<MessagesRequest> for WorkOrder {
    /// Convert an Anthropic Messages API request into an ABP [`WorkOrder`].
    ///
    /// The user message text becomes the work order task. Tools and metadata
    /// are stored in `config.vendor` for downstream adapters.
    fn from(req: MessagesRequest) -> Self {
        let task = extract_task(&req.messages);

        let mut vendor = BTreeMap::new();
        if let Some(tools) = &req.tools {
            if let Ok(v) = serde_json::to_value(tools) {
                vendor.insert("tools".into(), v);
            }
        }
        if let Some(meta) = &req.metadata {
            if let Ok(v) = serde_json::to_value(meta) {
                vendor.insert("metadata".into(), v);
            }
        }
        if let Some(sys) = &req.system {
            vendor.insert(
                "system".into(),
                serde_json::Value::String(system_message_text(sys)),
            );
        }

        let config = RuntimeConfig {
            model: Some(req.model.clone()),
            vendor,
            ..RuntimeConfig::default()
        };

        WorkOrderBuilder::new(task).config(config).build()
    }
}

impl From<Receipt> for MessagesResponse {
    /// Convert an ABP [`Receipt`] into an Anthropic Messages API response.
    ///
    /// Assistant text and tool-call events from the receipt trace are mapped
    /// to content blocks. The outcome is translated to a stop reason.
    fn from(receipt: Receipt) -> Self {
        let mut content_blocks = Vec::new();

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
                        let sig = event
                            .ext
                            .as_ref()
                            .and_then(|e| e.get("signature"))
                            .and_then(|v| v.as_str())
                            .map(String::from);
                        content_blocks.push(ContentBlock::Thinking {
                            thinking: text.clone(),
                            signature: sig,
                        });
                    } else {
                        content_blocks.push(ContentBlock::Text { text: text.clone() });
                    }
                }
                AgentEventKind::ToolCall {
                    tool_name,
                    tool_use_id,
                    input,
                    ..
                } => {
                    content_blocks.push(ContentBlock::ToolUse {
                        id: tool_use_id.clone().unwrap_or_default(),
                        name: tool_name.clone(),
                        input: input.clone(),
                    });
                }
                _ => {}
            }
        }

        // Derive stop reason from the last tool-call or outcome.
        let has_tool_use = content_blocks.iter().any(|b| {
            matches!(b, ContentBlock::ToolUse { .. })
        });
        let stop_reason = if has_tool_use {
            Some("tool_use".into())
        } else {
            match receipt.outcome {
                Outcome::Complete => Some("end_turn".into()),
                Outcome::Partial => Some("max_tokens".into()),
                Outcome::Failed => None,
            }
        };

        // Extract model from usage_raw if available, otherwise from backend id.
        let model = receipt
            .usage_raw
            .get("model")
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_else(|| receipt.backend.id.clone());

        let usage = Usage {
            input_tokens: receipt.usage.input_tokens.unwrap_or(0),
            output_tokens: receipt.usage.output_tokens.unwrap_or(0),
            cache_creation_input_tokens: receipt.usage.cache_write_tokens,
            cache_read_input_tokens: receipt.usage.cache_read_tokens,
        };

        MessagesResponse {
            id: format!("msg_{}", receipt.meta.run_id),
            response_type: "message".into(),
            role: "assistant".into(),
            content: content_blocks,
            model,
            stop_reason,
            stop_sequence: None,
            usage,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::{Outcome, ReceiptBuilder, UsageNormalized};

    // -- MessagesRequest serde roundtrips --

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
                    content: MessageContent::Blocks(vec![
                        ContentBlock::Text {
                            text: "Look at this image:".into(),
                        },
                        ContentBlock::Image {
                            source: ImageSource::Base64 {
                                media_type: "image/png".into(),
                                data: "iVBOR...".into(),
                            },
                        },
                    ]),
                },
                Message {
                    role: Role::Assistant,
                    content: MessageContent::Blocks(vec![ContentBlock::ToolUse {
                        id: "tu_1".into(),
                        name: "read_file".into(),
                        input: serde_json::json!({"path": "src/lib.rs"}),
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
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": { "path": { "type": "string" } },
                    "required": ["path"]
                }),
            }]),
            metadata: Some(Metadata {
                user_id: Some("user_123".into()),
            }),
            stream: Some(true),
            stop_sequences: Some(vec!["###".into()]),
            temperature: Some(0.7),
            top_p: Some(0.9),
            top_k: Some(40),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: MessagesRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, req);
    }

    #[test]
    fn messages_request_omits_none_fields() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![],
            max_tokens: 1024,
            system: None,
            tools: None,
            metadata: None,
            stream: None,
            stop_sequences: None,
            temperature: None,
            top_p: None,
            top_k: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.contains("system"));
        assert!(!json.contains("tools"));
        assert!(!json.contains("metadata"));
        assert!(!json.contains("stream"));
        assert!(!json.contains("stop_sequences"));
        assert!(!json.contains("temperature"));
    }

    // -- SystemMessage serde --

    #[test]
    fn system_message_string_serde() {
        let sys = SystemMessage::Text("You are helpful.".into());
        let json = serde_json::to_value(&sys).unwrap();
        assert_eq!(json, serde_json::Value::String("You are helpful.".into()));
        let parsed: SystemMessage = serde_json::from_value(json).unwrap();
        assert_eq!(parsed, sys);
    }

    #[test]
    fn system_message_blocks_serde() {
        let sys = SystemMessage::Blocks(vec![SystemBlock::Text {
            text: "System prompt".into(),
            cache_control: Some(CacheControl::ephemeral()),
        }]);
        let json = serde_json::to_string(&sys).unwrap();
        let parsed: SystemMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, sys);
    }

    // -- MessageContent serde --

    #[test]
    fn message_content_text_serde() {
        let content = MessageContent::Text("Hello".into());
        let json = serde_json::to_value(&content).unwrap();
        assert_eq!(json, serde_json::Value::String("Hello".into()));
        let parsed: MessageContent = serde_json::from_value(json).unwrap();
        assert_eq!(parsed, content);
    }

    #[test]
    fn message_content_blocks_serde() {
        let content = MessageContent::Blocks(vec![
            ContentBlock::Text {
                text: "Hello".into(),
            },
            ContentBlock::ToolUse {
                id: "tu_1".into(),
                name: "bash".into(),
                input: serde_json::json!({"command": "ls"}),
            },
        ]);
        let json = serde_json::to_string(&content).unwrap();
        let parsed: MessageContent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, content);
    }

    // -- Role serde --

    #[test]
    fn role_serde_roundtrip() {
        assert_eq!(serde_json::to_string(&Role::User).unwrap(), "\"user\"");
        assert_eq!(
            serde_json::to_string(&Role::Assistant).unwrap(),
            "\"assistant\""
        );
        let parsed: Role = serde_json::from_str("\"user\"").unwrap();
        assert_eq!(parsed, Role::User);
    }

    // -- MessagesResponse serde --

    #[test]
    fn messages_response_serde_roundtrip() {
        let resp = MessagesResponse {
            id: "msg_01XFDUDYJgAACzvnptvVoYEL".into(),
            response_type: "message".into(),
            role: "assistant".into(),
            content: vec![ContentBlock::Text {
                text: "Hello!".into(),
            }],
            model: "claude-sonnet-4-20250514".into(),
            stop_reason: Some("end_turn".into()),
            stop_sequence: None,
            usage: Usage {
                input_tokens: 25,
                output_tokens: 10,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: MessagesResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, resp);
    }

    #[test]
    fn messages_response_has_type_field() {
        let resp = MessagesResponse {
            id: "msg_test".into(),
            response_type: "message".into(),
            role: "assistant".into(),
            content: vec![],
            model: "claude-sonnet-4-20250514".into(),
            stop_reason: None,
            stop_sequence: None,
            usage: Usage {
                input_tokens: 0,
                output_tokens: 0,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["type"], "message");
        assert_eq!(json["role"], "assistant");
    }

    // -- Metadata serde --

    #[test]
    fn metadata_serde_roundtrip() {
        let meta = Metadata {
            user_id: Some("user_abc".into()),
        };
        let json = serde_json::to_string(&meta).unwrap();
        let parsed: Metadata = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, meta);
    }

    #[test]
    fn metadata_omits_none_user_id() {
        let meta = Metadata { user_id: None };
        let json = serde_json::to_string(&meta).unwrap();
        assert!(!json.contains("user_id"));
    }

    // -- From<MessagesRequest> for WorkOrder --

    #[test]
    fn messages_request_to_work_order_basic() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![Message {
                role: Role::User,
                content: MessageContent::Text("Fix the login bug".into()),
            }],
            max_tokens: 4096,
            system: None,
            tools: None,
            metadata: None,
            stream: None,
            stop_sequences: None,
            temperature: None,
            top_p: None,
            top_k: None,
        };
        let wo: WorkOrder = req.into();
        assert_eq!(wo.task, "Fix the login bug");
        assert_eq!(wo.config.model.as_deref(), Some("claude-sonnet-4-20250514"));
    }

    #[test]
    fn messages_request_to_work_order_with_tools() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![Message {
                role: Role::User,
                content: MessageContent::Text("Do something".into()),
            }],
            max_tokens: 4096,
            system: Some(SystemMessage::Text("Be precise.".into())),
            tools: Some(vec![Tool {
                name: "bash".into(),
                description: "Run a command".into(),
                input_schema: serde_json::json!({"type": "object"}),
            }]),
            metadata: Some(Metadata {
                user_id: Some("user_1".into()),
            }),
            stream: None,
            stop_sequences: None,
            temperature: None,
            top_p: None,
            top_k: None,
        };
        let wo: WorkOrder = req.into();
        assert_eq!(wo.task, "Do something");
        assert!(wo.config.vendor.contains_key("tools"));
        assert!(wo.config.vendor.contains_key("metadata"));
        assert!(wo.config.vendor.contains_key("system"));
        assert_eq!(
            wo.config.vendor["system"],
            serde_json::Value::String("Be precise.".into())
        );
    }

    #[test]
    fn messages_request_to_work_order_extracts_text_from_blocks() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![Message {
                role: Role::User,
                content: MessageContent::Blocks(vec![
                    ContentBlock::Text {
                        text: "First part.".into(),
                    },
                    ContentBlock::Image {
                        source: ImageSource::Url {
                            url: "https://example.com/img.png".into(),
                        },
                    },
                    ContentBlock::Text {
                        text: "Second part.".into(),
                    },
                ]),
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
        };
        let wo: WorkOrder = req.into();
        assert!(wo.task.contains("First part."));
        assert!(wo.task.contains("Second part."));
    }

    #[test]
    fn messages_request_to_work_order_multi_user_messages() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![
                Message {
                    role: Role::User,
                    content: MessageContent::Text("Hello".into()),
                },
                Message {
                    role: Role::Assistant,
                    content: MessageContent::Text("Hi there!".into()),
                },
                Message {
                    role: Role::User,
                    content: MessageContent::Text("Help me code".into()),
                },
            ],
            max_tokens: 4096,
            system: None,
            tools: None,
            metadata: None,
            stream: None,
            stop_sequences: None,
            temperature: None,
            top_p: None,
            top_k: None,
        };
        let wo: WorkOrder = req.into();
        assert!(wo.task.contains("Hello"));
        assert!(wo.task.contains("Help me code"));
        // Assistant messages are not part of the task
        assert!(!wo.task.contains("Hi there!"));
    }

    // -- From<Receipt> for MessagesResponse --

    #[test]
    fn receipt_to_messages_response_text_only() {
        let receipt = ReceiptBuilder::new("claude-sonnet-4-20250514")
            .outcome(Outcome::Complete)
            .usage(UsageNormalized {
                input_tokens: Some(100),
                output_tokens: Some(50),
                cache_read_tokens: None,
                cache_write_tokens: None,
                request_units: None,
                estimated_cost_usd: None,
            })
            .add_trace_event(abp_core::AgentEvent {
                ts: chrono::Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: "Hello, world!".into(),
                },
                ext: None,
            })
            .build();

        let resp: MessagesResponse = receipt.into();
        assert!(resp.id.starts_with("msg_"));
        assert_eq!(resp.response_type, "message");
        assert_eq!(resp.role, "assistant");
        assert_eq!(resp.content.len(), 1);
        assert!(matches!(
            &resp.content[0],
            ContentBlock::Text { text } if text == "Hello, world!"
        ));
        assert_eq!(resp.stop_reason.as_deref(), Some("end_turn"));
        assert_eq!(resp.usage.input_tokens, 100);
        assert_eq!(resp.usage.output_tokens, 50);
    }

    #[test]
    fn receipt_to_messages_response_with_tool_use() {
        let receipt = ReceiptBuilder::new("claude-sonnet-4-20250514")
            .outcome(Outcome::Complete)
            .add_trace_event(abp_core::AgentEvent {
                ts: chrono::Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: "Let me check.".into(),
                },
                ext: None,
            })
            .add_trace_event(abp_core::AgentEvent {
                ts: chrono::Utc::now(),
                kind: AgentEventKind::ToolCall {
                    tool_name: "read_file".into(),
                    tool_use_id: Some("toolu_01".into()),
                    parent_tool_use_id: None,
                    input: serde_json::json!({"path": "src/lib.rs"}),
                },
                ext: None,
            })
            .build();

        let resp: MessagesResponse = receipt.into();
        assert_eq!(resp.content.len(), 2);
        assert!(matches!(&resp.content[0], ContentBlock::Text { .. }));
        assert!(matches!(&resp.content[1], ContentBlock::ToolUse { name, .. } if name == "read_file"));
        // When tool_use blocks are present, stop_reason should be "tool_use"
        assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
    }

    #[test]
    fn receipt_to_messages_response_partial_outcome() {
        let receipt = ReceiptBuilder::new("claude-sonnet-4-20250514")
            .outcome(Outcome::Partial)
            .build();

        let resp: MessagesResponse = receipt.into();
        assert_eq!(resp.stop_reason.as_deref(), Some("max_tokens"));
    }

    #[test]
    fn receipt_to_messages_response_failed_outcome() {
        let receipt = ReceiptBuilder::new("claude-sonnet-4-20250514")
            .outcome(Outcome::Failed)
            .build();

        let resp: MessagesResponse = receipt.into();
        assert!(resp.stop_reason.is_none());
    }

    #[test]
    fn receipt_to_messages_response_with_cache_tokens() {
        let receipt = ReceiptBuilder::new("claude-sonnet-4-20250514")
            .outcome(Outcome::Complete)
            .usage(UsageNormalized {
                input_tokens: Some(200),
                output_tokens: Some(100),
                cache_read_tokens: Some(50),
                cache_write_tokens: Some(30),
                request_units: None,
                estimated_cost_usd: None,
            })
            .build();

        let resp: MessagesResponse = receipt.into();
        assert_eq!(resp.usage.input_tokens, 200);
        assert_eq!(resp.usage.output_tokens, 100);
        assert_eq!(resp.usage.cache_read_input_tokens, Some(50));
        assert_eq!(resp.usage.cache_creation_input_tokens, Some(30));
    }

    #[test]
    fn receipt_to_messages_response_extracts_model_from_usage_raw() {
        let receipt = ReceiptBuilder::new("sidecar:claude")
            .outcome(Outcome::Complete)
            .usage_raw(serde_json::json!({
                "model": "claude-sonnet-4-20250514",
                "input_tokens": 100
            }))
            .build();

        let resp: MessagesResponse = receipt.into();
        assert_eq!(resp.model, "claude-sonnet-4-20250514");
    }

    #[test]
    fn receipt_to_messages_response_thinking_blocks() {
        let mut ext = BTreeMap::new();
        ext.insert("thinking".into(), serde_json::Value::Bool(true));
        ext.insert(
            "signature".into(),
            serde_json::Value::String("sig_abc".into()),
        );

        let receipt = ReceiptBuilder::new("claude-sonnet-4-20250514")
            .outcome(Outcome::Complete)
            .add_trace_event(abp_core::AgentEvent {
                ts: chrono::Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: "Let me think...".into(),
                },
                ext: Some(ext),
            })
            .add_trace_event(abp_core::AgentEvent {
                ts: chrono::Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: "Here is the answer.".into(),
                },
                ext: None,
            })
            .build();

        let resp: MessagesResponse = receipt.into();
        assert_eq!(resp.content.len(), 2);
        assert!(matches!(
            &resp.content[0],
            ContentBlock::Thinking { thinking, signature }
            if thinking == "Let me think..." && signature.as_deref() == Some("sig_abc")
        ));
        assert!(matches!(
            &resp.content[1],
            ContentBlock::Text { text } if text == "Here is the answer."
        ));
    }

    // -- Tool serde (re-exported ClaudeToolDef) --

    #[test]
    fn tool_serde_roundtrip() {
        let tool = Tool {
            name: "bash".into(),
            description: "Execute a bash command".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" }
                },
                "required": ["command"]
            }),
        };
        let json = serde_json::to_string(&tool).unwrap();
        let parsed: Tool = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, tool);
    }

    #[test]
    fn tool_json_uses_input_schema_field() {
        let tool = Tool {
            name: "test".into(),
            description: "Test tool".into(),
            input_schema: serde_json::json!({"type": "object"}),
        };
        let json = serde_json::to_value(&tool).unwrap();
        assert!(json.get("input_schema").is_some());
        assert!(json.get("parameters_schema").is_none());
    }

    // -- Usage serde (re-exported ClaudeUsage) --

    #[test]
    fn usage_full_serde_roundtrip() {
        let usage = Usage {
            input_tokens: 500,
            output_tokens: 250,
            cache_creation_input_tokens: Some(100),
            cache_read_input_tokens: Some(50),
        };
        let json = serde_json::to_string(&usage).unwrap();
        let parsed: Usage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, usage);
    }

    // -- StreamEvent serde (re-exported ClaudeStreamEvent) --

    #[test]
    fn stream_event_message_start_serde() {
        let event = StreamEvent::MessageStart {
            message: crate::dialect::ClaudeResponse {
                id: "msg_1".into(),
                model: "claude-sonnet-4-20250514".into(),
                role: "assistant".into(),
                content: vec![],
                stop_reason: None,
                usage: None,
            },
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("message_start"));
        let parsed: StreamEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, event);
    }

    #[test]
    fn stream_event_content_block_delta_serde() {
        let event = StreamEvent::ContentBlockDelta {
            index: 0,
            delta: StreamDelta::TextDelta {
                text: "Hello".into(),
            },
        };
        let json = serde_json::to_string(&event).unwrap();
        let parsed: StreamEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, event);
    }

    #[test]
    fn stream_event_message_delta_serde() {
        let event = StreamEvent::MessageDelta {
            delta: MessageDelta {
                stop_reason: Some("end_turn".into()),
                stop_sequence: None,
            },
            usage: Some(Usage {
                input_tokens: 0,
                output_tokens: 42,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            }),
        };
        let json = serde_json::to_string(&event).unwrap();
        let parsed: StreamEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, event);
    }

    #[test]
    fn stream_event_message_stop_serde() {
        let event = StreamEvent::MessageStop {};
        let json = serde_json::to_string(&event).unwrap();
        let parsed: StreamEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, event);
    }
}
