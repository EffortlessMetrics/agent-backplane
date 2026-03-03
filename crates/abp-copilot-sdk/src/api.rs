// SPDX-License-Identifier: MIT OR Apache-2.0
//! Public GitHub Copilot Extensions API surface types and conversions to/from ABP core.
//!
//! This module provides first-class Rust types matching the GitHub Copilot
//! Extensions API surface (OpenAI-compatible with Copilot-specific extensions),
//! plus `From` trait conversions to map between these types and
//! [`abp_core::WorkOrder`] / [`abp_core::Receipt`].

use abp_core::{AgentEventKind, Outcome, Receipt, WorkOrder, WorkOrderBuilder};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::dialect::{CopilotConfirmation, CopilotReference, CopilotReferenceType};

// ---------------------------------------------------------------------------
// Message types
// ---------------------------------------------------------------------------

/// A message in the Copilot Extensions conversation format.
///
/// Compatible with the OpenAI Chat format, extended with Copilot-specific
/// fields such as `copilot_references`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CopilotMessage {
    /// Message role (`system`, `user`, or `assistant`).
    pub role: String,
    /// Text content of the message.
    pub content: String,
    /// Optional display name for the message author.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Optional function call requested by the assistant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function_call: Option<CopilotFunctionCall>,
    /// Copilot-specific context references attached to this message.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub copilot_references: Vec<CopilotReference>,
}

/// A function call within a Copilot message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CopilotFunctionCall {
    /// Name of the function to invoke.
    pub name: String,
    /// JSON-encoded arguments string.
    pub arguments: String,
}

// ---------------------------------------------------------------------------
// Reference types (re-exported from dialect for API surface)
// ---------------------------------------------------------------------------

// `CopilotReference` and `CopilotReferenceType` are re-exported from
// `crate::dialect` via the `use` import above.

// ---------------------------------------------------------------------------
// Confirmation types (re-exported from dialect for API surface)
// ---------------------------------------------------------------------------

// `CopilotConfirmation` is re-exported from `crate::dialect` above.

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

/// A request to the GitHub Copilot Extensions API.
///
/// Uses the OpenAI Chat Completions format with Copilot-specific extensions
/// such as `copilot_references` on messages and top-level `references`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CopilotRequest {
    /// Model identifier (e.g. `gpt-4o`).
    pub model: String,
    /// Conversation messages.
    pub messages: Vec<CopilotMessage>,
    /// Whether to stream the response via SSE.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    /// Sampling temperature (0.0–2.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Maximum tokens to generate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Top-level Copilot references for the request.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<CopilotReference>,
    /// Copilot-specific metadata (headers, agent info, etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copilot_metadata: Option<BTreeMap<String, serde_json::Value>>,
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// Token usage statistics (OpenAI-compatible).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CopilotUsage {
    /// Tokens consumed by the prompt.
    pub prompt_tokens: u64,
    /// Tokens generated in the completion.
    pub completion_tokens: u64,
    /// Total tokens (prompt + completion).
    pub total_tokens: u64,
}

/// The reason a model stopped generating tokens.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CopilotFinishReason {
    /// Natural stop or hit a stop sequence.
    Stop,
    /// Token limit reached.
    Length,
    /// Model decided to call function(s).
    FunctionCall,
    /// Content was omitted due to content filter.
    ContentFilter,
}

/// The assistant message inside a response choice.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CopilotAssistantMessage {
    /// Role — always `"assistant"`.
    #[serde(default = "default_assistant_role")]
    pub role: String,
    /// Text content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Function call requested by the assistant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function_call: Option<CopilotFunctionCall>,
    /// Copilot references emitted with this message.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub copilot_references: Vec<CopilotReference>,
}

fn default_assistant_role() -> String {
    "assistant".into()
}

/// A single choice in the Copilot response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CopilotChoice {
    /// Zero-based index of this choice.
    pub index: u32,
    /// The assistant's response message.
    pub message: CopilotAssistantMessage,
    /// Reason the model stopped generating.
    pub finish_reason: CopilotFinishReason,
}

/// A response from the GitHub Copilot Extensions API.
///
/// Uses the OpenAI Chat Completions response format with Copilot-specific
/// extensions such as `copilot_references` and `copilot_confirmation`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CopilotResponse {
    /// Unique response identifier.
    pub id: String,
    /// Object type — always `"chat.completion"`.
    pub object: String,
    /// Unix timestamp of creation.
    pub created: u64,
    /// Model that generated the completion.
    pub model: String,
    /// Completion choices.
    pub choices: Vec<CopilotChoice>,
    /// Token usage statistics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<CopilotUsage>,
    /// Copilot confirmation prompt (if the agent requests user approval).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copilot_confirmation: Option<CopilotConfirmation>,
}

// ---------------------------------------------------------------------------
// From<CopilotRequest> for WorkOrder
// ---------------------------------------------------------------------------

impl From<CopilotRequest> for WorkOrder {
    fn from(req: CopilotRequest) -> Self {
        // Extract the last user message as the task.
        let task = req
            .messages
            .iter()
            .rev()
            .find_map(|m| {
                if m.role == "user" {
                    Some(m.content.clone())
                } else {
                    None
                }
            })
            .unwrap_or_default();

        // Collect system messages as context snippets.
        let snippets: Vec<abp_core::ContextSnippet> = req
            .messages
            .iter()
            .enumerate()
            .filter_map(|(i, m)| {
                if m.role == "system" {
                    Some(abp_core::ContextSnippet {
                        name: format!("system_{i}"),
                        content: m.content.clone(),
                    })
                } else {
                    None
                }
            })
            .collect();

        // Collect top-level file references as context files.
        let files: Vec<String> = req
            .references
            .iter()
            .filter(|r| r.ref_type == CopilotReferenceType::File)
            .filter_map(|r| {
                r.data
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(String::from)
            })
            .collect();

        let mut builder = WorkOrderBuilder::new(task).model(&req.model);

        if !snippets.is_empty() || !files.is_empty() {
            builder = builder.context(abp_core::ContextPacket { files, snippets });
        }

        builder.build()
    }
}

// ---------------------------------------------------------------------------
// From<Receipt> for CopilotResponse
// ---------------------------------------------------------------------------

impl From<Receipt> for CopilotResponse {
    fn from(receipt: Receipt) -> Self {
        let mut text_parts = Vec::new();
        let mut function_call = None;
        let mut finish = CopilotFinishReason::Stop;
        let mut references = Vec::new();
        let mut confirmation = None;

        for event in &receipt.trace {
            match &event.kind {
                AgentEventKind::AssistantMessage { text } => {
                    text_parts.push(text.clone());
                }
                AgentEventKind::ToolCall {
                    tool_name, input, ..
                } => {
                    finish = CopilotFinishReason::FunctionCall;
                    function_call = Some(CopilotFunctionCall {
                        name: tool_name.clone(),
                        arguments: serde_json::to_string(input).unwrap_or_default(),
                    });
                }
                AgentEventKind::Warning { .. } => {
                    // Check ext for copilot_confirmation
                    if let Some(ext) = &event.ext {
                        if let Some(conf_val) = ext.get("copilot_confirmation") {
                            if let Ok(conf) =
                                serde_json::from_value::<CopilotConfirmation>(conf_val.clone())
                            {
                                confirmation = Some(conf);
                            }
                        }
                    }
                }
                AgentEventKind::Error { .. } => {
                    finish = CopilotFinishReason::Stop;
                }
                _ => {
                    // Check ext for copilot_references
                    if let Some(ext) = &event.ext {
                        if let Some(refs_val) = ext.get("copilot_references") {
                            if let Ok(refs) =
                                serde_json::from_value::<Vec<CopilotReference>>(refs_val.clone())
                            {
                                references.extend(refs);
                            }
                        }
                    }
                }
            }
        }

        if receipt.outcome == Outcome::Failed {
            finish = CopilotFinishReason::Stop;
        }

        let content = if text_parts.is_empty() {
            None
        } else {
            Some(text_parts.join(""))
        };

        let usage = {
            let u = &receipt.usage;
            if u.input_tokens.is_some() || u.output_tokens.is_some() {
                let input = u.input_tokens.unwrap_or(0);
                let output = u.output_tokens.unwrap_or(0);
                Some(CopilotUsage {
                    prompt_tokens: input,
                    completion_tokens: output,
                    total_tokens: input + output,
                })
            } else {
                None
            }
        };

        let created = receipt.meta.finished_at.timestamp() as u64;

        CopilotResponse {
            id: format!("copilot-{}", receipt.meta.run_id),
            object: "chat.completion".into(),
            created,
            model: receipt.backend.id.clone(),
            choices: vec![CopilotChoice {
                index: 0,
                message: CopilotAssistantMessage {
                    role: "assistant".into(),
                    content,
                    function_call,
                    copilot_references: references,
                },
                finish_reason: finish,
            }],
            usage,
            copilot_confirmation: confirmation,
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
        AgentEvent, BackendIdentity, CapabilityManifest, ExecutionMode, Outcome, RunMetadata,
        UsageNormalized, VerificationReport,
    };
    use chrono::Utc;
    use serde_json::json;
    use uuid::Uuid;

    // ── Serde roundtrips ────────────────────────────────────────────────

    #[test]
    fn message_user_serde_roundtrip() {
        let msg = CopilotMessage {
            role: "user".into(),
            content: "Hello!".into(),
            name: None,
            function_call: None,
            copilot_references: vec![],
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""role":"user""#));
        let parsed: CopilotMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, msg);
    }

    #[test]
    fn message_system_serde_roundtrip() {
        let msg = CopilotMessage {
            role: "system".into(),
            content: "You are helpful.".into(),
            name: None,
            function_call: None,
            copilot_references: vec![],
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""role":"system""#));
        let parsed: CopilotMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, msg);
    }

    #[test]
    fn message_assistant_serde_roundtrip() {
        let msg = CopilotMessage {
            role: "assistant".into(),
            content: "Sure!".into(),
            name: Some("copilot".into()),
            function_call: None,
            copilot_references: vec![],
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""role":"assistant""#));
        assert!(json.contains(r#""name":"copilot""#));
        let parsed: CopilotMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, msg);
    }

    #[test]
    fn message_with_function_call_serde_roundtrip() {
        let msg = CopilotMessage {
            role: "assistant".into(),
            content: String::new(),
            name: None,
            function_call: Some(CopilotFunctionCall {
                name: "read_file".into(),
                arguments: r#"{"path":"main.rs"}"#.into(),
            }),
            copilot_references: vec![],
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("read_file"));
        let parsed: CopilotMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, msg);
    }

    #[test]
    fn message_with_references_serde_roundtrip() {
        let msg = CopilotMessage {
            role: "user".into(),
            content: "Check this file".into(),
            name: None,
            function_call: None,
            copilot_references: vec![CopilotReference {
                ref_type: CopilotReferenceType::File,
                id: "file-0".into(),
                data: json!({"path": "src/main.rs"}),
                metadata: None,
            }],
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("copilot_references"));
        let parsed: CopilotMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, msg);
    }

    #[test]
    fn message_omits_empty_optional_fields() {
        let msg = CopilotMessage {
            role: "user".into(),
            content: "Hello".into(),
            name: None,
            function_call: None,
            copilot_references: vec![],
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(!json.contains("name"));
        assert!(!json.contains("function_call"));
        assert!(!json.contains("copilot_references"));
    }

    #[test]
    fn reference_serde_roundtrip() {
        let r = CopilotReference {
            ref_type: CopilotReferenceType::Repository,
            id: "repo-0".into(),
            data: json!({"owner": "octocat", "name": "hello-world"}),
            metadata: Some({
                let mut m = BTreeMap::new();
                m.insert(
                    "url".into(),
                    json!("https://github.com/octocat/hello-world"),
                );
                m
            }),
        };
        let json = serde_json::to_string(&r).unwrap();
        let parsed: CopilotReference = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, r);
    }

    #[test]
    fn confirmation_serde_roundtrip() {
        let c = CopilotConfirmation {
            id: "conf-1".into(),
            title: "Delete file".into(),
            message: "Are you sure you want to delete main.rs?".into(),
            accepted: Some(true),
        };
        let json = serde_json::to_string(&c).unwrap();
        let parsed: CopilotConfirmation = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, c);
    }

    #[test]
    fn finish_reason_serde_roundtrip() {
        for (reason, expected_str) in [
            (CopilotFinishReason::Stop, "\"stop\""),
            (CopilotFinishReason::Length, "\"length\""),
            (CopilotFinishReason::FunctionCall, "\"function_call\""),
            (CopilotFinishReason::ContentFilter, "\"content_filter\""),
        ] {
            let json = serde_json::to_string(&reason).unwrap();
            assert_eq!(json, expected_str);
            let parsed: CopilotFinishReason = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, reason);
        }
    }

    #[test]
    fn usage_serde_roundtrip() {
        let usage = CopilotUsage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
        };
        let json = serde_json::to_string(&usage).unwrap();
        let parsed: CopilotUsage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, usage);
    }

    // ── Request serde ───────────────────────────────────────────────────

    #[test]
    fn request_full_serde_roundtrip() {
        let req = CopilotRequest {
            model: "gpt-4o".into(),
            messages: vec![
                CopilotMessage {
                    role: "system".into(),
                    content: "Be helpful.".into(),
                    name: None,
                    function_call: None,
                    copilot_references: vec![],
                },
                CopilotMessage {
                    role: "user".into(),
                    content: "Hello".into(),
                    name: None,
                    function_call: None,
                    copilot_references: vec![],
                },
            ],
            stream: Some(true),
            temperature: Some(0.7),
            max_tokens: Some(4096),
            references: vec![CopilotReference {
                ref_type: CopilotReferenceType::File,
                id: "f-0".into(),
                data: json!({"path": "src/lib.rs"}),
                metadata: None,
            }],
            copilot_metadata: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: CopilotRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, req);
    }

    #[test]
    fn request_omits_none_fields() {
        let req = CopilotRequest {
            model: "gpt-4o".into(),
            messages: vec![],
            stream: None,
            temperature: None,
            max_tokens: None,
            references: vec![],
            copilot_metadata: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.contains("stream"));
        assert!(!json.contains("temperature"));
        assert!(!json.contains("max_tokens"));
        assert!(!json.contains("references"));
        assert!(!json.contains("copilot_metadata"));
    }

    // ── Response serde ──────────────────────────────────────────────────

    #[test]
    fn response_serde_roundtrip() {
        let resp = CopilotResponse {
            id: "copilot-abc".into(),
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

    // ── From<CopilotRequest> for WorkOrder ──────────────────────────────

    fn make_request(messages: Vec<CopilotMessage>) -> CopilotRequest {
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

    #[test]
    fn request_to_work_order_uses_last_user_message_as_task() {
        let req = make_request(vec![
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
    fn request_to_work_order_preserves_model() {
        let mut req = make_request(vec![CopilotMessage {
            role: "user".into(),
            content: "Hello".into(),
            name: None,
            function_call: None,
            copilot_references: vec![],
        }]);
        req.model = "gpt-4-turbo".into();
        let wo: WorkOrder = req.into();
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4-turbo"));
    }

    #[test]
    fn request_to_work_order_maps_system_to_snippets() {
        let req = make_request(vec![
            CopilotMessage {
                role: "system".into(),
                content: "Be concise.".into(),
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
        assert_eq!(wo.context.snippets[0].content, "Be concise.");
    }

    #[test]
    fn request_to_work_order_empty_messages_yields_empty_task() {
        let req = make_request(vec![]);
        let wo: WorkOrder = req.into();
        assert_eq!(wo.task, "");
    }

    #[test]
    fn request_to_work_order_maps_file_references_to_context() {
        let mut req = make_request(vec![CopilotMessage {
            role: "user".into(),
            content: "Check files".into(),
            name: None,
            function_call: None,
            copilot_references: vec![],
        }]);
        req.references = vec![
            CopilotReference {
                ref_type: CopilotReferenceType::File,
                id: "f-0".into(),
                data: json!({"path": "src/main.rs"}),
                metadata: None,
            },
            CopilotReference {
                ref_type: CopilotReferenceType::Repository,
                id: "r-0".into(),
                data: json!({"owner": "test"}),
                metadata: None,
            },
        ];
        let wo: WorkOrder = req.into();
        assert_eq!(wo.context.files.len(), 1);
        assert_eq!(wo.context.files[0], "src/main.rs");
    }

    // ── From<Receipt> for CopilotResponse ───────────────────────────────

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
                id: "copilot/gpt-4o".into(),
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
        let resp: CopilotResponse = receipt.into();

        assert_eq!(resp.object, "chat.completion");
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Hello there!")
        );
        assert_eq!(resp.choices[0].finish_reason, CopilotFinishReason::Stop);
    }

    #[test]
    fn receipt_to_response_maps_tool_call_as_function_call() {
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
        let resp: CopilotResponse = receipt.into();

        assert_eq!(
            resp.choices[0].finish_reason,
            CopilotFinishReason::FunctionCall
        );
        let fc = resp.choices[0].message.function_call.as_ref().unwrap();
        assert_eq!(fc.name, "read_file");
    }

    #[test]
    fn receipt_to_response_maps_usage() {
        let usage = UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            ..UsageNormalized::default()
        };
        let receipt = make_receipt(vec![], usage);
        let resp: CopilotResponse = receipt.into();

        let u = resp.usage.unwrap();
        assert_eq!(u.prompt_tokens, 100);
        assert_eq!(u.completion_tokens, 50);
        assert_eq!(u.total_tokens, 150);
    }

    #[test]
    fn receipt_to_response_no_usage_when_both_none() {
        let receipt = make_receipt(vec![], UsageNormalized::default());
        let resp: CopilotResponse = receipt.into();
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
        let resp: CopilotResponse = receipt.into();
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Part 1. Part 2.")
        );
    }

    #[test]
    fn receipt_to_response_failed_outcome() {
        let mut receipt = make_receipt(vec![], UsageNormalized::default());
        receipt.outcome = Outcome::Failed;
        let resp: CopilotResponse = receipt.into();
        assert_eq!(resp.choices[0].finish_reason, CopilotFinishReason::Stop);
    }

    #[test]
    fn receipt_to_response_id_contains_run_id() {
        let receipt = make_receipt(vec![], UsageNormalized::default());
        let run_id = receipt.meta.run_id;
        let resp: CopilotResponse = receipt.into();
        assert!(resp.id.contains(&run_id.to_string()));
        assert!(resp.id.starts_with("copilot-"));
    }

    #[test]
    fn receipt_to_response_mixed_text_and_function_call() {
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
        let resp: CopilotResponse = receipt.into();

        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Let me check.")
        );
        assert!(resp.choices[0].message.function_call.is_some());
        assert_eq!(
            resp.choices[0].finish_reason,
            CopilotFinishReason::FunctionCall
        );
    }

    #[test]
    fn receipt_to_response_extracts_confirmation_from_ext() {
        let mut ext = BTreeMap::new();
        ext.insert(
            "copilot_confirmation".into(),
            json!({
                "id": "conf-1",
                "title": "Delete file",
                "message": "Delete main.rs?"
            }),
        );
        let trace = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Warning {
                message: "Confirmation required: Delete file".into(),
            },
            ext: Some(ext),
        }];
        let receipt = make_receipt(trace, UsageNormalized::default());
        let resp: CopilotResponse = receipt.into();

        let conf = resp.copilot_confirmation.unwrap();
        assert_eq!(conf.title, "Delete file");
        assert_eq!(conf.message, "Delete main.rs?");
    }
}
