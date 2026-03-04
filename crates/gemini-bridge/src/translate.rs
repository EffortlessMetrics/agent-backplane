// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code, unused_imports)]
//! Translation between Gemini GenerateContent API types and ABP contract types.
//!
//! This module is gated behind the `normalized` feature because it depends
//! on `abp-core` for contract types.

#[cfg(feature = "normalized")]
mod inner {
    use std::collections::BTreeMap;

    use abp_core::{
        AgentEvent, AgentEventKind, ContextPacket, ContextSnippet, Outcome, Receipt,
        RuntimeConfig, UsageNormalized, WorkOrder, WorkOrderBuilder,
    };
    use chrono::Utc;

    use crate::gemini_types::{
        Candidate, Content, FunctionCall, FunctionResponse, GenerateContentRequest,
        GenerateContentResponse, GenerationConfig, GeminiTool, HarmCategory, InlineData, Part,
        PromptFeedback, SafetyRating, SafetySetting, StreamGenerateContentResponse, UsageMetadata,
    };
    use crate::error::BridgeError;

    // ── Role mapping ────────────────────────────────────────────────────

    /// Map a Gemini wire-role (`"user"` / `"model"`) to the ABP canonical form.
    #[must_use]
    pub fn gemini_role_to_canonical(role: &str) -> &'static str {
        match role {
            "model" => "assistant",
            "user" => "user",
            _ => "user",
        }
    }

    /// Map an ABP canonical role back to the Gemini wire-role.
    #[must_use]
    pub fn canonical_role_to_gemini(role: &str) -> &'static str {
        match role {
            "assistant" | "model" => "model",
            "system" => "user",
            _ => "user",
        }
    }

    // ── gemini_to_work_order ────────────────────────────────────────────

    /// Convert a Gemini [`GenerateContentRequest`] into an ABP [`WorkOrder`].
    ///
    /// # Mapping rules
    ///
    /// | Gemini field           | WorkOrder field                          |
    /// |------------------------|------------------------------------------|
    /// | Last user text part    | `task`                                   |
    /// | `system_instruction`   | `context.snippets`                       |
    /// | `contents`             | `config.vendor["contents"]`              |
    /// | `tools`                | `config.vendor["tools"]`                 |
    /// | `generation_config`    | `config.vendor["generation_config"]`     |
    /// | `safety_settings`      | `config.vendor["safety_settings"]`       |
    /// | `model`                | `config.model`                           |
    #[must_use]
    pub fn gemini_to_work_order(req: &GenerateContentRequest) -> WorkOrder {
        // Extract the last user-role text part as the task.
        let task = req
            .contents
            .iter()
            .rev()
            .filter(|c| c.role.as_deref() == Some("user"))
            .flat_map(|c| c.parts.iter())
            .find_map(|p| match p {
                Part::Text(t) => Some(t.clone()),
                _ => None,
            })
            .unwrap_or_default();

        // Collect system instruction text as context snippets.
        let snippets: Vec<ContextSnippet> = req
            .system_instruction
            .iter()
            .flat_map(|sys| sys.parts.iter())
            .filter_map(|p| match p {
                Part::Text(t) => Some(ContextSnippet {
                    name: "system_instruction".into(),
                    content: t.clone(),
                }),
                _ => None,
            })
            .collect();

        let mut vendor = BTreeMap::new();
        vendor.insert("dialect".into(), serde_json::Value::String("gemini".into()));

        if let Ok(v) = serde_json::to_value(&req.contents) {
            vendor.insert("contents".into(), v);
        }
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

        let config = RuntimeConfig {
            model: Some(req.model.clone()),
            vendor,
            ..RuntimeConfig::default()
        };

        let mut builder = WorkOrderBuilder::new(task).config(config);

        if !snippets.is_empty() {
            builder = builder.context(ContextPacket {
                files: vec![],
                snippets,
            });
        }

        builder.build()
    }

    // ── receipt_to_gemini ───────────────────────────────────────────────

    /// Map an ABP [`Outcome`] to a Gemini `finishReason` string.
    #[must_use]
    pub fn outcome_to_finish_reason(outcome: &Outcome) -> &'static str {
        match outcome {
            Outcome::Complete => "STOP",
            Outcome::Partial => "MAX_TOKENS",
            Outcome::Failed => "OTHER",
        }
    }

    /// Convert ABP normalized usage to Gemini [`UsageMetadata`].
    #[must_use]
    fn usage_to_metadata(usage: &UsageNormalized) -> Option<UsageMetadata> {
        if usage.input_tokens.is_some() || usage.output_tokens.is_some() {
            let input = usage.input_tokens.unwrap_or(0);
            let output = usage.output_tokens.unwrap_or(0);
            Some(UsageMetadata {
                prompt_token_count: input,
                candidates_token_count: output,
                total_token_count: input + output,
            })
        } else {
            None
        }
    }

    /// Convert an ABP [`Receipt`] into a Gemini [`GenerateContentResponse`].
    #[must_use]
    pub fn receipt_to_gemini(receipt: &Receipt) -> GenerateContentResponse {
        let mut parts = Vec::new();

        for event in &receipt.trace {
            match &event.kind {
                AgentEventKind::AssistantMessage { text } => {
                    parts.push(Part::Text(text.clone()));
                }
                AgentEventKind::ToolCall {
                    tool_name, input, ..
                } => {
                    parts.push(Part::FunctionCall(FunctionCall {
                        name: tool_name.clone(),
                        args: input.clone(),
                    }));
                }
                AgentEventKind::ToolResult {
                    tool_name, output, ..
                } => {
                    parts.push(Part::FunctionResponse(FunctionResponse {
                        name: tool_name.clone(),
                        response: output.clone(),
                    }));
                }
                _ => {}
            }
        }

        let finish_reason = outcome_to_finish_reason(&receipt.outcome);

        let candidate = Candidate {
            content: Content {
                role: Some("model".into()),
                parts,
            },
            finish_reason: Some(finish_reason.into()),
            safety_ratings: None,
            citation_metadata: None,
        };

        let usage_metadata = usage_to_metadata(&receipt.usage);

        GenerateContentResponse {
            candidates: vec![candidate],
            usage_metadata,
            prompt_feedback: None,
        }
    }

    // ── agent_event_to_gemini_stream ────────────────────────────────────

    /// Convert a single ABP [`AgentEvent`] to a Gemini streaming chunk.
    ///
    /// Returns `None` for event kinds that have no Gemini SSE representation.
    #[must_use]
    pub fn agent_event_to_gemini_stream(
        event: &AgentEvent,
    ) -> Option<StreamGenerateContentResponse> {
        match &event.kind {
            AgentEventKind::AssistantDelta { text } => {
                Some(StreamGenerateContentResponse {
                    candidates: vec![Candidate {
                        content: Content {
                            role: Some("model".into()),
                            parts: vec![Part::Text(text.clone())],
                        },
                        finish_reason: None,
                        safety_ratings: None,
                        citation_metadata: None,
                    }],
                    usage_metadata: None,
                })
            }
            AgentEventKind::AssistantMessage { text } => {
                Some(StreamGenerateContentResponse {
                    candidates: vec![Candidate {
                        content: Content {
                            role: Some("model".into()),
                            parts: vec![Part::Text(text.clone())],
                        },
                        finish_reason: Some("STOP".into()),
                        safety_ratings: None,
                        citation_metadata: None,
                    }],
                    usage_metadata: None,
                })
            }
            AgentEventKind::ToolCall {
                tool_name, input, ..
            } => {
                Some(StreamGenerateContentResponse {
                    candidates: vec![Candidate {
                        content: Content {
                            role: Some("model".into()),
                            parts: vec![Part::FunctionCall(FunctionCall {
                                name: tool_name.clone(),
                                args: input.clone(),
                            })],
                        },
                        finish_reason: None,
                        safety_ratings: None,
                        citation_metadata: None,
                    }],
                    usage_metadata: None,
                })
            }
            AgentEventKind::RunCompleted { .. } => {
                Some(StreamGenerateContentResponse {
                    candidates: vec![Candidate {
                        content: Content {
                            role: Some("model".into()),
                            parts: vec![],
                        },
                        finish_reason: Some("STOP".into()),
                        safety_ratings: None,
                        citation_metadata: None,
                    }],
                    usage_metadata: None,
                })
            }
            _ => None,
        }
    }

    // ── Error translation ───────────────────────────────────────────────

    /// Convert a Gemini API error into a [`BridgeError`].
    pub fn api_error_to_bridge(
        error: &crate::gemini_types::GeminiErrorResponse,
    ) -> BridgeError {
        let detail = &error.error;
        match detail.code {
            401 => BridgeError::Config(format!("authentication failed: {}", detail.message)),
            400 => BridgeError::Config(format!("invalid request: {}", detail.message)),
            429 => BridgeError::Run(format!("rate limited: {}", detail.message)),
            503 => BridgeError::Run(format!("API overloaded: {}", detail.message)),
            _ => BridgeError::Run(format!("API error {}: {}", detail.code, detail.message)),
        }
    }

    // ── Request construction helpers ────────────────────────────────────

    /// Build a minimal [`GenerateContentRequest`] from a task string.
    pub fn task_to_request(task: &str, model: &str) -> GenerateContentRequest {
        GenerateContentRequest {
            model: model.to_string(),
            contents: vec![Content::user(vec![Part::Text(task.to_string())])],
            tools: None,
            generation_config: None,
            safety_settings: None,
            system_instruction: None,
        }
    }
}

#[cfg(feature = "normalized")]
pub use inner::*;
