// SPDX-License-Identifier: MIT OR Apache-2.0
//! Public OpenAI Codex / Responses API surface types and conversions to/from ABP core.
//!
//! This module provides first-class Rust types matching the OpenAI Responses API
//! surface used by Codex, plus `From` trait conversions to map between these types
//! and [`abp_core::WorkOrder`] / [`abp_core::Receipt`].

use abp_core::{AgentEventKind, Outcome, Receipt, WorkOrder, WorkOrderBuilder};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

/// A Codex / Responses API session request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CodexSessionRequest {
    /// Model identifier (e.g. `codex-mini-latest`).
    pub model: String,
    /// System-level instructions for the model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// Input items (text messages, function call results).
    pub input: Vec<CodexInputItem>,
    /// Tool definitions available to the model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<CodexTool>>,
    /// Whether to stream the response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    /// ID of a previous response to continue a multi-turn session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_response_id: Option<String>,
    /// Maximum output tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    /// Sampling temperature (0.0–2.0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
}

/// An input item in a Codex session request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CodexInputItem {
    /// A conversation message.
    Message {
        /// Message role (`user`, `assistant`, or `system`).
        role: String,
        /// Text content of the message.
        content: String,
    },
    /// The output of a previously invoked function call.
    FunctionCallOutput {
        /// Correlation ID linking back to the function call.
        call_id: String,
        /// The output value from the function.
        output: String,
    },
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// A Codex / Responses API session response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CodexSessionResponse {
    /// Unique response identifier (e.g. `resp_...`).
    pub id: String,
    /// Object type — always `"response"`.
    pub object: String,
    /// Response status (`completed`, `in_progress`, `failed`, `cancelled`).
    pub status: String,
    /// Output items produced by the model.
    pub output: Vec<CodexOutputItem>,
    /// Token usage statistics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<CodexUsage>,
    /// Model that generated the response.
    pub model: String,
}

/// An output item in a Codex session response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CodexOutputItem {
    /// An assistant message with content parts.
    Message {
        /// Message role (always `"assistant"`).
        role: String,
        /// Content parts of the message.
        content: Vec<CodexContentPart>,
    },
    /// A function call requested by the model.
    FunctionCall {
        /// Unique function call identifier.
        id: String,
        /// Correlation ID linking the call to a prior request.
        #[serde(skip_serializing_if = "Option::is_none")]
        call_id: Option<String>,
        /// Name of the function to invoke.
        name: String,
        /// JSON-encoded arguments.
        arguments: String,
    },
}

/// A content part within a Codex output message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CodexContentPart {
    /// Text output from the model.
    OutputText {
        /// The text content.
        text: String,
    },
}

/// Token usage reported by the Codex / Responses API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CodexUsage {
    /// Tokens consumed by the input.
    pub input_tokens: u64,
    /// Tokens generated in the output.
    pub output_tokens: u64,
    /// Total tokens (input + output).
    pub total_tokens: u64,
}

// ---------------------------------------------------------------------------
// Tool types
// ---------------------------------------------------------------------------

/// A tool definition for the Codex / Responses API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CodexTool {
    /// Tool type — typically `"function"`.
    #[serde(rename = "type")]
    pub tool_type: String,
    /// The function definition.
    pub function: CodexFunctionDef,
}

/// A function definition inside a [`CodexTool`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CodexFunctionDef {
    /// Function name.
    pub name: String,
    /// Human-readable description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// JSON Schema for the function parameters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// From<CodexSessionRequest> for WorkOrder
// ---------------------------------------------------------------------------

impl From<CodexSessionRequest> for WorkOrder {
    fn from(req: CodexSessionRequest) -> Self {
        // Extract the last user message as the task.
        let task = req
            .input
            .iter()
            .rev()
            .find_map(|item| match item {
                CodexInputItem::Message { role, content, .. } if role == "user" => {
                    Some(content.clone())
                }
                _ => None,
            })
            .unwrap_or_default();

        // Collect instructions as a context snippet.
        let mut snippets: Vec<abp_core::ContextSnippet> = Vec::new();
        if let Some(instructions) = &req.instructions {
            snippets.push(abp_core::ContextSnippet {
                name: "instructions".into(),
                content: instructions.clone(),
            });
        }

        // Collect system messages as context snippets.
        for (i, item) in req.input.iter().enumerate() {
            if let CodexInputItem::Message { role, content, .. } = item {
                if role == "system" {
                    snippets.push(abp_core::ContextSnippet {
                        name: format!("system_{i}"),
                        content: content.clone(),
                    });
                }
            }
        }

        let mut builder = WorkOrderBuilder::new(task).model(&req.model);

        if !snippets.is_empty() {
            builder = builder.context(abp_core::ContextPacket {
                files: vec![],
                snippets,
            });
        }

        // Store vendor-specific fields for roundtrip fidelity.
        let mut vendor = BTreeMap::new();
        if let Some(prev) = &req.previous_response_id {
            vendor.insert(
                "previous_response_id".into(),
                serde_json::Value::String(prev.clone()),
            );
        }
        if let Some(stream) = req.stream {
            vendor.insert("stream".into(), serde_json::Value::Bool(stream));
        }

        if !vendor.is_empty() {
            let config = abp_core::RuntimeConfig {
                model: Some(req.model.clone()),
                vendor,
                ..Default::default()
            };
            builder = builder.config(config);
        }

        builder.build()
    }
}

// ---------------------------------------------------------------------------
// From<Receipt> for CodexSessionResponse
// ---------------------------------------------------------------------------

impl From<Receipt> for CodexSessionResponse {
    fn from(receipt: Receipt) -> Self {
        let mut output = Vec::new();
        let mut text_parts = Vec::new();

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
                    // Flush accumulated text as a message item.
                    if !text_parts.is_empty() {
                        output.push(CodexOutputItem::Message {
                            role: "assistant".into(),
                            content: vec![CodexContentPart::OutputText {
                                text: text_parts.join(""),
                            }],
                        });
                        text_parts.clear();
                    }
                    output.push(CodexOutputItem::FunctionCall {
                        id: tool_use_id
                            .clone()
                            .unwrap_or_else(|| format!("call_{}", output.len())),
                        call_id: None,
                        name: tool_name.clone(),
                        arguments: serde_json::to_string(input).unwrap_or_default(),
                    });
                }
                _ => {}
            }
        }

        // Flush remaining text.
        if !text_parts.is_empty() {
            output.push(CodexOutputItem::Message {
                role: "assistant".into(),
                content: vec![CodexContentPart::OutputText {
                    text: text_parts.join(""),
                }],
            });
        }

        let status = match receipt.outcome {
            Outcome::Complete => "completed",
            Outcome::Partial => "incomplete",
            Outcome::Failed => "failed",
        }
        .to_string();

        let usage = {
            let u = &receipt.usage;
            if u.input_tokens.is_some() || u.output_tokens.is_some() {
                let input = u.input_tokens.unwrap_or(0);
                let output_toks = u.output_tokens.unwrap_or(0);
                Some(CodexUsage {
                    input_tokens: input,
                    output_tokens: output_toks,
                    total_tokens: input + output_toks,
                })
            } else {
                None
            }
        };

        CodexSessionResponse {
            id: format!("resp_{}", receipt.meta.run_id),
            object: "response".into(),
            status,
            output,
            usage,
            model: receipt.backend.id.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::{
        AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, ExecutionMode, Outcome,
        RunMetadata, UsageNormalized, VerificationReport,
    };
    use chrono::Utc;
    use serde_json::json;
    use uuid::Uuid;

    // ── Serde roundtrips ────────────────────────────────────────────────

    #[test]
    fn input_item_message_serde_roundtrip() {
        let item = CodexInputItem::Message {
            role: "user".into(),
            content: "Hello!".into(),
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains(r#""type":"message""#));
        let parsed: CodexInputItem = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, item);
    }

    #[test]
    fn input_item_function_call_output_serde_roundtrip() {
        let item = CodexInputItem::FunctionCallOutput {
            call_id: "call_123".into(),
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
    fn output_item_function_call_no_call_id_omits_field() {
        let item = CodexOutputItem::FunctionCall {
            id: "fc_2".into(),
            call_id: None,
            name: "read".into(),
            arguments: "{}".into(),
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(!json.contains("call_id"));
        let parsed: CodexOutputItem = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, item);
    }

    #[test]
    fn content_part_output_text_serde_roundtrip() {
        let part = CodexContentPart::OutputText {
            text: "Hello world".into(),
        };
        let json = serde_json::to_string(&part).unwrap();
        assert!(json.contains(r#""type":"output_text""#));
        let parsed: CodexContentPart = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, part);
    }

    #[test]
    fn codex_usage_serde_roundtrip() {
        let usage = CodexUsage {
            input_tokens: 100,
            output_tokens: 50,
            total_tokens: 150,
        };
        let json = serde_json::to_string(&usage).unwrap();
        let parsed: CodexUsage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, usage);
    }

    #[test]
    fn codex_tool_serde_roundtrip() {
        let tool = CodexTool {
            tool_type: "function".into(),
            function: CodexFunctionDef {
                name: "get_weather".into(),
                description: Some("Get weather".into()),
                parameters: Some(json!({
                    "type": "object",
                    "properties": { "location": { "type": "string" } },
                    "required": ["location"]
                })),
            },
        };
        let json = serde_json::to_string(&tool).unwrap();
        assert!(json.contains(r#""type":"function""#));
        let parsed: CodexTool = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, tool);
    }

    #[test]
    fn codex_tool_minimal_serde_roundtrip() {
        let tool = CodexTool {
            tool_type: "function".into(),
            function: CodexFunctionDef {
                name: "noop".into(),
                description: None,
                parameters: None,
            },
        };
        let json = serde_json::to_string(&tool).unwrap();
        assert!(!json.contains("description"));
        assert!(!json.contains("parameters"));
        let parsed: CodexTool = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, tool);
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
        let req = CodexSessionRequest {
            model: "codex-mini-latest".into(),
            instructions: None,
            input: vec![],
            tools: None,
            stream: None,
            previous_response_id: None,
            max_output_tokens: None,
            temperature: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.contains("instructions"));
        assert!(!json.contains("tools"));
        assert!(!json.contains("stream"));
        assert!(!json.contains("previous_response_id"));
        assert!(!json.contains("max_output_tokens"));
        assert!(!json.contains("temperature"));
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
                    text: "Done!".into(),
                }],
            }],
            usage: Some(CodexUsage {
                input_tokens: 10,
                output_tokens: 5,
                total_tokens: 15,
            }),
            model: "codex-mini-latest".into(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: CodexSessionResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, resp);
    }

    #[test]
    fn session_response_no_usage_roundtrip() {
        let resp = CodexSessionResponse {
            id: "resp_xyz".into(),
            object: "response".into(),
            status: "completed".into(),
            output: vec![],
            usage: None,
            model: "codex-mini-latest".into(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(!json.contains("usage"));
        let parsed: CodexSessionResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, resp);
    }

    // ── From<CodexSessionRequest> for WorkOrder ─────────────────────────

    fn make_request(input: Vec<CodexInputItem>) -> CodexSessionRequest {
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

    #[test]
    fn request_to_work_order_uses_last_user_message_as_task() {
        let req = make_request(vec![
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
    fn request_to_work_order_preserves_model() {
        let mut req = make_request(vec![CodexInputItem::Message {
            role: "user".into(),
            content: "Hello".into(),
        }]);
        req.model = "o4-mini".into();
        let wo: WorkOrder = req.into();
        assert_eq!(wo.config.model.as_deref(), Some("o4-mini"));
    }

    #[test]
    fn request_to_work_order_maps_instructions_to_snippet() {
        let mut req = make_request(vec![CodexInputItem::Message {
            role: "user".into(),
            content: "Fix it".into(),
        }]);
        req.instructions = Some("Be concise.".into());
        let wo: WorkOrder = req.into();
        assert_eq!(wo.context.snippets.len(), 1);
        assert_eq!(wo.context.snippets[0].name, "instructions");
        assert_eq!(wo.context.snippets[0].content, "Be concise.");
    }

    #[test]
    fn request_to_work_order_maps_system_messages_to_snippets() {
        let req = make_request(vec![
            CodexInputItem::Message {
                role: "system".into(),
                content: "System prompt".into(),
            },
            CodexInputItem::Message {
                role: "user".into(),
                content: "Hi".into(),
            },
        ]);
        let wo: WorkOrder = req.into();
        assert_eq!(wo.context.snippets.len(), 1);
        assert_eq!(wo.context.snippets[0].content, "System prompt");
    }

    #[test]
    fn request_to_work_order_empty_input_yields_empty_task() {
        let req = make_request(vec![]);
        let wo: WorkOrder = req.into();
        assert_eq!(wo.task, "");
    }

    #[test]
    fn request_to_work_order_ignores_function_call_output_for_task() {
        let req = make_request(vec![CodexInputItem::FunctionCallOutput {
            call_id: "fc_1".into(),
            output: "result".into(),
        }]);
        let wo: WorkOrder = req.into();
        assert_eq!(wo.task, "");
    }

    #[test]
    fn request_to_work_order_previous_response_id_in_vendor() {
        let mut req = make_request(vec![CodexInputItem::Message {
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

    // ── From<Receipt> for CodexSessionResponse ──────────────────────────

    fn make_receipt(trace: Vec<AgentEvent>, usage: UsageNormalized) -> Receipt {
        let now = Utc::now();
        Receipt {
            meta: RunMetadata {
                run_id: Uuid::new_v4(),
                work_order_id: Uuid::new_v4(),
                contract_version: "abp/v0.1".into(),
                started_at: now,
                finished_at: now,
                duration_ms: 100,
            },
            backend: BackendIdentity {
                id: "openai/codex-mini-latest".into(),
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
        let resp: CodexSessionResponse = receipt.into();

        assert_eq!(resp.object, "response");
        assert_eq!(resp.status, "completed");
        assert_eq!(resp.output.len(), 1);
        match &resp.output[0] {
            CodexOutputItem::Message { role, content } => {
                assert_eq!(role, "assistant");
                assert_eq!(content.len(), 1);
                match &content[0] {
                    CodexContentPart::OutputText { text } => assert_eq!(text, "Hello there!"),
                }
            }
            other => panic!("expected Message, got {other:?}"),
        }
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
        let resp: CodexSessionResponse = receipt.into();

        assert_eq!(resp.output.len(), 1);
        match &resp.output[0] {
            CodexOutputItem::FunctionCall {
                id,
                name,
                arguments,
                ..
            } => {
                assert_eq!(id, "call_abc");
                assert_eq!(name, "read_file");
                assert!(arguments.contains("main.rs"));
            }
            other => panic!("expected FunctionCall, got {other:?}"),
        }
    }

    #[test]
    fn receipt_to_response_maps_usage() {
        let usage = UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            ..UsageNormalized::default()
        };
        let receipt = make_receipt(vec![], usage);
        let resp: CodexSessionResponse = receipt.into();

        let u = resp.usage.unwrap();
        assert_eq!(u.input_tokens, 100);
        assert_eq!(u.output_tokens, 50);
        assert_eq!(u.total_tokens, 150);
    }

    #[test]
    fn receipt_to_response_no_usage_when_both_none() {
        let receipt = make_receipt(vec![], UsageNormalized::default());
        let resp: CodexSessionResponse = receipt.into();
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
        let resp: CodexSessionResponse = receipt.into();

        assert_eq!(resp.output.len(), 1);
        match &resp.output[0] {
            CodexOutputItem::Message { content, .. } => {
                assert_eq!(content.len(), 1);
                match &content[0] {
                    CodexContentPart::OutputText { text } => {
                        assert_eq!(text, "Part 1. Part 2.");
                    }
                }
            }
            other => panic!("expected Message, got {other:?}"),
        }
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
        let resp: CodexSessionResponse = receipt.into();

        assert_eq!(resp.output.len(), 2);
        assert!(matches!(&resp.output[0], CodexOutputItem::Message { .. }));
        assert!(matches!(
            &resp.output[1],
            CodexOutputItem::FunctionCall { .. }
        ));
    }

    #[test]
    fn receipt_to_response_failed_outcome() {
        let mut receipt = make_receipt(vec![], UsageNormalized::default());
        receipt.outcome = Outcome::Failed;
        let resp: CodexSessionResponse = receipt.into();
        assert_eq!(resp.status, "failed");
    }

    #[test]
    fn receipt_to_response_partial_outcome() {
        let mut receipt = make_receipt(vec![], UsageNormalized::default());
        receipt.outcome = Outcome::Partial;
        let resp: CodexSessionResponse = receipt.into();
        assert_eq!(resp.status, "incomplete");
    }

    #[test]
    fn receipt_to_response_id_contains_run_id() {
        let receipt = make_receipt(vec![], UsageNormalized::default());
        let run_id = receipt.meta.run_id;
        let resp: CodexSessionResponse = receipt.into();
        assert!(resp.id.contains(&run_id.to_string()));
        assert!(resp.id.starts_with("resp_"));
    }

    #[test]
    fn receipt_to_response_empty_trace() {
        let receipt = make_receipt(vec![], UsageNormalized::default());
        let resp: CodexSessionResponse = receipt.into();
        assert!(resp.output.is_empty());
        assert_eq!(resp.status, "completed");
    }

    #[test]
    fn btreemap_deterministic_vendor_serialization() {
        let mut req = make_request(vec![CodexInputItem::Message {
            role: "user".into(),
            content: "test".into(),
        }]);
        req.previous_response_id = Some("resp_1".into());
        req.stream = Some(true);
        let wo: WorkOrder = req.into();
        let json1 = serde_json::to_string(&wo.config.vendor).unwrap();
        let json2 = serde_json::to_string(&wo.config.vendor).unwrap();
        assert_eq!(json1, json2);
    }
}
