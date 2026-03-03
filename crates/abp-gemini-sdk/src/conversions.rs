// SPDX-License-Identifier: MIT OR Apache-2.0
//! Conversions between Gemini API types and ABP core types.
//!
//! Implements [`From<GeminiRequest>`] for [`WorkOrder`] and
//! [`From<Receipt>`] for [`GeminiResponse`].

use std::collections::BTreeMap;

use abp_core::{
    AgentEventKind, Outcome, Receipt, RuntimeConfig, WorkOrder, WorkOrderBuilder,
};

use crate::dialect::{
    GeminiCandidate, GeminiContent, GeminiPart, GeminiRequest, GeminiResponse,
    GeminiUsageMetadata,
};

// ---------------------------------------------------------------------------
// From<GeminiRequest> for WorkOrder
// ---------------------------------------------------------------------------

impl From<GeminiRequest> for WorkOrder {
    /// Convert a Gemini [`GeminiRequest`] into an ABP [`WorkOrder`].
    ///
    /// The last user-role text content is used as the work order task.
    /// System instructions are stored as context snippets, and tools/config
    /// are preserved in `config.vendor` for downstream adapters.
    fn from(req: GeminiRequest) -> Self {
        // Extract the last user text as the task.
        let task = req
            .contents
            .iter()
            .rev()
            .filter(|c| c.role == "user")
            .flat_map(|c| c.parts.iter())
            .find_map(|p| match p {
                GeminiPart::Text(t) => Some(t.clone()),
                _ => None,
            })
            .unwrap_or_default();

        // Collect system instruction as a context snippet.
        let snippets: Vec<abp_core::ContextSnippet> = req
            .system_instruction
            .iter()
            .flat_map(|sys| sys.parts.iter())
            .filter_map(|p| match p {
                GeminiPart::Text(t) => Some(abp_core::ContextSnippet {
                    name: "system_instruction".into(),
                    content: t.clone(),
                }),
                _ => None,
            })
            .collect();

        let mut vendor = BTreeMap::new();
        if let Some(tools) = &req.tools {
            if let Ok(v) = serde_json::to_value(tools) {
                vendor.insert("tools".into(), v);
            }
        }
        if let Some(gen_cfg) = &req.generation_config {
            if let Ok(v) = serde_json::to_value(gen_cfg) {
                vendor.insert("generation_config".into(), v);
            }
        }
        if let Some(safety) = &req.safety_settings {
            if let Ok(v) = serde_json::to_value(safety) {
                vendor.insert("safety_settings".into(), v);
            }
        }
        if let Some(tool_cfg) = &req.tool_config {
            if let Ok(v) = serde_json::to_value(tool_cfg) {
                vendor.insert("tool_config".into(), v);
            }
        }

        let config = RuntimeConfig {
            model: Some(req.model.clone()),
            vendor,
            ..RuntimeConfig::default()
        };

        let mut builder = WorkOrderBuilder::new(task).config(config);

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
// From<Receipt> for GeminiResponse
// ---------------------------------------------------------------------------

impl From<Receipt> for GeminiResponse {
    /// Convert an ABP [`Receipt`] into a Gemini [`GeminiResponse`].
    ///
    /// Assistant text and tool-call events from the receipt trace are mapped
    /// to Gemini content parts. The outcome determines the finish reason.
    fn from(receipt: Receipt) -> Self {
        let mut parts = Vec::new();

        for event in &receipt.trace {
            match &event.kind {
                AgentEventKind::AssistantMessage { text } => {
                    parts.push(GeminiPart::Text(text.clone()));
                }
                AgentEventKind::ToolCall {
                    tool_name, input, ..
                } => {
                    parts.push(GeminiPart::FunctionCall {
                        name: tool_name.clone(),
                        args: input.clone(),
                    });
                }
                AgentEventKind::ToolResult {
                    tool_name, output, ..
                } => {
                    parts.push(GeminiPart::FunctionResponse {
                        name: tool_name.clone(),
                        response: output.clone(),
                    });
                }
                _ => {}
            }
        }

        let finish_reason = match receipt.outcome {
            Outcome::Complete => Some("STOP".into()),
            Outcome::Partial => Some("MAX_TOKENS".into()),
            Outcome::Failed => Some("OTHER".into()),
        };

        let candidate = GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts,
            },
            finish_reason,
            safety_ratings: None,
            citation_metadata: None,
        };

        let usage_metadata = {
            let u = &receipt.usage;
            if u.input_tokens.is_some() || u.output_tokens.is_some() {
                let input = u.input_tokens.unwrap_or(0);
                let output = u.output_tokens.unwrap_or(0);
                Some(GeminiUsageMetadata {
                    prompt_token_count: input,
                    candidates_token_count: output,
                    total_token_count: input + output,
                })
            } else {
                None
            }
        };

        GeminiResponse {
            candidates: vec![candidate],
            prompt_feedback: None,
            usage_metadata,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialect::{
        GeminiGenerationConfig, GeminiSafetySetting, GeminiTool,
        GeminiFunctionDeclaration, HarmBlockThreshold, HarmCategory,
    };
    use abp_core::{Outcome, ReceiptBuilder};

    // ── From<GeminiRequest> for WorkOrder ───────────────────────────────

    #[test]
    fn request_to_work_order_extracts_task() {
        let req = GeminiRequest {
            model: "gemini-2.5-flash".into(),
            contents: vec![GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::Text("Explain Rust traits".into())],
            }],
            system_instruction: None,
            generation_config: None,
            safety_settings: None,
            tools: None,
            tool_config: None,
        };
        let wo: WorkOrder = req.into();
        assert_eq!(wo.task, "Explain Rust traits");
        assert_eq!(wo.config.model.as_deref(), Some("gemini-2.5-flash"));
    }

    #[test]
    fn request_to_work_order_uses_last_user_text() {
        let req = GeminiRequest {
            model: "gemini-2.5-flash".into(),
            contents: vec![
                GeminiContent {
                    role: "user".into(),
                    parts: vec![GeminiPart::Text("First message".into())],
                },
                GeminiContent {
                    role: "model".into(),
                    parts: vec![GeminiPart::Text("Response".into())],
                },
                GeminiContent {
                    role: "user".into(),
                    parts: vec![GeminiPart::Text("Second message".into())],
                },
            ],
            system_instruction: None,
            generation_config: None,
            safety_settings: None,
            tools: None,
            tool_config: None,
        };
        let wo: WorkOrder = req.into();
        assert_eq!(wo.task, "Second message");
    }

    #[test]
    fn request_to_work_order_with_system_instruction() {
        let req = GeminiRequest {
            model: "gemini-2.5-flash".into(),
            contents: vec![GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::Text("Hello".into())],
            }],
            system_instruction: Some(GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::Text("Be concise.".into())],
            }),
            generation_config: None,
            safety_settings: None,
            tools: None,
            tool_config: None,
        };
        let wo: WorkOrder = req.into();
        assert_eq!(wo.context.snippets.len(), 1);
        assert_eq!(wo.context.snippets[0].content, "Be concise.");
    }

    #[test]
    fn request_to_work_order_preserves_tools_in_vendor() {
        let req = GeminiRequest {
            model: "gemini-2.5-flash".into(),
            contents: vec![GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::Text("Search".into())],
            }],
            system_instruction: None,
            generation_config: Some(GeminiGenerationConfig {
                temperature: Some(0.5),
                ..Default::default()
            }),
            safety_settings: Some(vec![GeminiSafetySetting {
                category: HarmCategory::HarmCategoryHarassment,
                threshold: HarmBlockThreshold::BlockNone,
            }]),
            tools: Some(vec![GeminiTool {
                function_declarations: vec![GeminiFunctionDeclaration {
                    name: "search".into(),
                    description: "Search".into(),
                    parameters: serde_json::json!({"type": "object"}),
                }],
            }]),
            tool_config: None,
        };
        let wo: WorkOrder = req.into();
        assert!(wo.config.vendor.contains_key("tools"));
        assert!(wo.config.vendor.contains_key("generation_config"));
        assert!(wo.config.vendor.contains_key("safety_settings"));
    }

    #[test]
    fn request_to_work_order_empty_contents() {
        let req = GeminiRequest {
            model: "gemini-2.5-flash".into(),
            contents: vec![],
            system_instruction: None,
            generation_config: None,
            safety_settings: None,
            tools: None,
            tool_config: None,
        };
        let wo: WorkOrder = req.into();
        assert!(wo.task.is_empty());
    }

    // ── From<Receipt> for GeminiResponse ────────────────────────────────

    #[test]
    fn receipt_to_response_text() {
        let receipt = ReceiptBuilder::new("gemini")
            .outcome(Outcome::Complete)
            .add_trace_event(abp_core::AgentEvent {
                ts: chrono::Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: "Hello there!".into(),
                },
                ext: None,
            })
            .build();
        let resp: GeminiResponse = receipt.into();
        assert_eq!(resp.candidates.len(), 1);
        assert_eq!(resp.candidates[0].finish_reason.as_deref(), Some("STOP"));
        match &resp.candidates[0].content.parts[0] {
            GeminiPart::Text(t) => assert_eq!(t, "Hello there!"),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn receipt_to_response_tool_call() {
        let receipt = ReceiptBuilder::new("gemini")
            .outcome(Outcome::Complete)
            .add_trace_event(abp_core::AgentEvent {
                ts: chrono::Utc::now(),
                kind: AgentEventKind::ToolCall {
                    tool_name: "search".into(),
                    tool_use_id: None,
                    parent_tool_use_id: None,
                    input: serde_json::json!({"q": "rust"}),
                },
                ext: None,
            })
            .build();
        let resp: GeminiResponse = receipt.into();
        match &resp.candidates[0].content.parts[0] {
            GeminiPart::FunctionCall { name, args } => {
                assert_eq!(name, "search");
                assert_eq!(args["q"], "rust");
            }
            other => panic!("expected FunctionCall, got {other:?}"),
        }
    }

    #[test]
    fn receipt_to_response_tool_result() {
        let receipt = ReceiptBuilder::new("gemini")
            .outcome(Outcome::Complete)
            .add_trace_event(abp_core::AgentEvent {
                ts: chrono::Utc::now(),
                kind: AgentEventKind::ToolResult {
                    tool_name: "search".into(),
                    tool_use_id: None,
                    output: serde_json::json!({"results": []}),
                    is_error: false,
                },
                ext: None,
            })
            .build();
        let resp: GeminiResponse = receipt.into();
        match &resp.candidates[0].content.parts[0] {
            GeminiPart::FunctionResponse { name, response } => {
                assert_eq!(name, "search");
                assert_eq!(response, &serde_json::json!({"results": []}));
            }
            other => panic!("expected FunctionResponse, got {other:?}"),
        }
    }

    #[test]
    fn receipt_to_response_partial_outcome() {
        let receipt = ReceiptBuilder::new("gemini")
            .outcome(Outcome::Partial)
            .build();
        let resp: GeminiResponse = receipt.into();
        assert_eq!(
            resp.candidates[0].finish_reason.as_deref(),
            Some("MAX_TOKENS")
        );
    }

    #[test]
    fn receipt_to_response_failed_outcome() {
        let receipt = ReceiptBuilder::new("gemini")
            .outcome(Outcome::Failed)
            .build();
        let resp: GeminiResponse = receipt.into();
        assert_eq!(resp.candidates[0].finish_reason.as_deref(), Some("OTHER"));
    }

    #[test]
    fn receipt_to_response_with_usage() {
        let usage = abp_core::UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            ..Default::default()
        };
        let receipt = ReceiptBuilder::new("gemini")
            .outcome(Outcome::Complete)
            .usage(usage)
            .build();
        let resp: GeminiResponse = receipt.into();
        let meta = resp.usage_metadata.unwrap();
        assert_eq!(meta.prompt_token_count, 100);
        assert_eq!(meta.candidates_token_count, 50);
        assert_eq!(meta.total_token_count, 150);
    }

    #[test]
    fn receipt_to_response_no_usage() {
        let receipt = ReceiptBuilder::new("gemini")
            .outcome(Outcome::Complete)
            .build();
        let resp: GeminiResponse = receipt.into();
        assert!(resp.usage_metadata.is_none());
    }

    #[test]
    fn receipt_to_response_no_prompt_feedback() {
        let receipt = ReceiptBuilder::new("gemini")
            .outcome(Outcome::Complete)
            .build();
        let resp: GeminiResponse = receipt.into();
        assert!(resp.prompt_feedback.is_none());
    }

    // ── Roundtrip: Request → WorkOrder → Receipt → Response ─────────────

    #[test]
    fn roundtrip_request_to_response() {
        let req = GeminiRequest {
            model: "gemini-2.5-flash".into(),
            contents: vec![GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::Text("Hello".into())],
            }],
            system_instruction: None,
            generation_config: None,
            safety_settings: None,
            tools: None,
            tool_config: None,
        };

        // Request → WorkOrder
        let wo: WorkOrder = req.into();
        assert_eq!(wo.task, "Hello");

        // Build a receipt as if the backend processed the work order
        let receipt = ReceiptBuilder::new("gemini")
            .outcome(Outcome::Complete)
            .work_order_id(wo.id)
            .add_trace_event(abp_core::AgentEvent {
                ts: chrono::Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: "Hi!".into(),
                },
                ext: None,
            })
            .build();

        // Receipt → Response
        let resp: GeminiResponse = receipt.into();
        assert_eq!(resp.candidates.len(), 1);
        match &resp.candidates[0].content.parts[0] {
            GeminiPart::Text(t) => assert_eq!(t, "Hi!"),
            other => panic!("expected Text, got {other:?}"),
        }
    }
}
