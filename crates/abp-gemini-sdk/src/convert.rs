// SPDX-License-Identifier: MIT OR Apache-2.0
//! Conversion layer between Gemini `GenerateContent` API types and ABP contract types.
//!
//! Provides three main entry points:
//!
//! - [`to_work_order`](crate::convert::to_work_order) — convert an incoming Gemini request into an ABP `WorkOrder`.
//! - [`from_receipt`](crate::convert::from_receipt) — project an ABP `Receipt` back into a Gemini response.
//! - [`from_agent_event`](crate::convert::from_agent_event) — map a single streaming `AgentEvent` to a Gemini SSE JSON value.
//!
//! Helper functions for role mapping and part-type conversion are also exposed.

use std::collections::BTreeMap;

use abp_core::{
    AgentEvent, AgentEventKind, ContextPacket, ContextSnippet, Outcome, Receipt, RuntimeConfig,
    WorkOrder, WorkOrderBuilder,
};

use crate::types::{
    Candidate, Content, GenerateContentRequest, GenerateContentResponse, GenerationConfig, Part,
    UsageMetadata,
};

// ---------------------------------------------------------------------------
// Role mapping helpers
// ---------------------------------------------------------------------------

/// Map a Gemini wire-role (`"user"` / `"model"`) to the ABP canonical form
/// (`"user"` / `"assistant"`).
///
/// Unknown roles are passed through unchanged.
#[must_use]
pub fn gemini_role_to_canonical(role: &str) -> &'static str {
    match role {
        "model" => "assistant",
        "user" => "user",
        _ => "user",
    }
}

/// Map an ABP canonical role (`"user"` / `"assistant"`) back to the Gemini
/// wire-role (`"user"` / `"model"`).
///
/// Unknown roles default to `"user"`.
#[must_use]
pub fn canonical_role_to_gemini(role: &str) -> &'static str {
    match role {
        "assistant" | "model" => "model",
        "system" => "user",
        _ => "user",
    }
}

// ---------------------------------------------------------------------------
// Part conversion helpers
// ---------------------------------------------------------------------------

/// Serialize a Gemini [`Part`] into a JSON value for storage in vendor config.
#[must_use]
pub fn part_to_json(part: &Part) -> serde_json::Value {
    serde_json::to_value(part).unwrap_or(serde_json::Value::Null)
}

/// Extract plain text from a Gemini [`Part`], returning `None` for non-text parts.
#[must_use]
pub fn part_text(part: &Part) -> Option<&str> {
    match part {
        Part::Text(t) => Some(t.as_str()),
        _ => None,
    }
}

/// Build a Gemini text [`Part`] from a string.
#[must_use]
pub fn text_part(text: impl Into<String>) -> Part {
    Part::Text(text.into())
}

/// Build a Gemini function-call [`Part`].
#[must_use]
pub fn function_call_part(name: impl Into<String>, args: serde_json::Value) -> Part {
    Part::FunctionCall {
        name: name.into(),
        args,
    }
}

/// Build a Gemini function-response [`Part`].
#[must_use]
pub fn function_response_part(name: impl Into<String>, response: serde_json::Value) -> Part {
    Part::FunctionResponse {
        name: name.into(),
        response,
    }
}

// ---------------------------------------------------------------------------
// to_work_order
// ---------------------------------------------------------------------------

/// Convert a Gemini [`GenerateContentRequest`] into an ABP [`WorkOrder`].
///
/// # Mapping rules
///
/// | Gemini field           | WorkOrder field                          |
/// |------------------------|------------------------------------------|
/// | Last user text part    | `task`                                   |
/// | `system_instruction`   | `context.snippets` (name = `"system_instruction"`) |
/// | `contents`             | `config.vendor["contents"]`              |
/// | `tools`                | `config.vendor["tools"]`                 |
/// | `generation_config`    | `config.vendor["generation_config"]`     |
/// | `safety_settings`      | `config.vendor["safety_settings"]`       |
/// | `tool_config`          | `config.vendor["tool_config"]`           |
///
/// The dialect is recorded as `config.vendor["dialect"] = "gemini"`.
///
/// Gemini `GenerateContentRequest` (from `crate::types`) does not carry a
/// model field (the model lives in the URL path), so `config.model` is left
/// as `None`.
#[must_use]
pub fn to_work_order(req: &GenerateContentRequest) -> WorkOrder {
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

    // Tag dialect.
    vendor.insert("dialect".into(), serde_json::Value::String("gemini".into()));

    // Preserve full contents for round-tripping.
    if let Ok(v) = serde_json::to_value(&req.contents) {
        vendor.insert("contents".into(), v);
    }

    // Preserve tools.
    if let Some(tools) = &req.tools {
        if let Ok(v) = serde_json::to_value(tools) {
            vendor.insert("tools".into(), v);
        }
    }

    // Preserve generation_config.
    if let Some(gen_cfg) = &req.generation_config {
        if let Ok(v) = serde_json::to_value(gen_cfg) {
            vendor.insert("generation_config".into(), v);
        }
    }

    // Preserve safety_settings.
    if let Some(safety) = &req.safety_settings {
        if let Ok(v) = serde_json::to_value(safety) {
            vendor.insert("safety_settings".into(), v);
        }
    }

    // Preserve tool_config.
    if let Some(tool_cfg) = &req.tool_config {
        if let Ok(v) = serde_json::to_value(tool_cfg) {
            vendor.insert("tool_config".into(), v);
        }
    }

    let config = RuntimeConfig {
        model: None,
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

// ---------------------------------------------------------------------------
// from_receipt
// ---------------------------------------------------------------------------

/// Convert an ABP [`Receipt`] back into a Gemini [`GenerateContentResponse`].
///
/// The `wo` parameter is accepted for future use (e.g. correlating request
/// metadata) but is not consumed in the current implementation.
///
/// # Mapping rules
///
/// | Receipt field                        | Gemini response field                     |
/// |--------------------------------------|-------------------------------------------|
/// | `trace[AssistantMessage]`            | `candidates[0].content.parts` (text)      |
/// | `trace[ToolCall]`                    | `candidates[0].content.parts` (functionCall) |
/// | `trace[ToolResult]`                  | `candidates[0].content.parts` (functionResponse) |
/// | `outcome`                            | `candidates[0].finish_reason`             |
/// | `usage.input_tokens / output_tokens` | `usage_metadata`                          |
#[must_use]
pub fn from_receipt(receipt: &Receipt, _wo: &WorkOrder) -> GenerateContentResponse {
    let mut parts = Vec::new();

    for event in &receipt.trace {
        match &event.kind {
            AgentEventKind::AssistantMessage { text } => {
                parts.push(Part::Text(text.clone()));
            }
            AgentEventKind::ToolCall {
                tool_name, input, ..
            } => {
                parts.push(Part::FunctionCall {
                    name: tool_name.clone(),
                    args: input.clone(),
                });
            }
            AgentEventKind::ToolResult {
                tool_name, output, ..
            } => {
                parts.push(Part::FunctionResponse {
                    name: tool_name.clone(),
                    response: output.clone(),
                });
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
    };

    let usage_metadata = usage_to_metadata(&receipt.usage);

    GenerateContentResponse {
        candidates: vec![candidate],
        usage_metadata,
        prompt_feedback: None,
    }
}

/// Map an ABP [`Outcome`] to a Gemini `finishReason` string.
#[must_use]
pub fn outcome_to_finish_reason(outcome: &Outcome) -> &'static str {
    match outcome {
        Outcome::Complete => "STOP",
        Outcome::Partial => "MAX_TOKENS",
        Outcome::Failed => "OTHER",
    }
}

/// Map ABP normalized usage to Gemini [`UsageMetadata`], if any token counts
/// are available.
#[must_use]
fn usage_to_metadata(usage: &abp_core::UsageNormalized) -> Option<UsageMetadata> {
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

// ---------------------------------------------------------------------------
// from_agent_event
// ---------------------------------------------------------------------------

/// Convert a single ABP [`AgentEvent`] to a Gemini-compatible SSE JSON value.
///
/// Returns `None` for event kinds that have no Gemini SSE representation
/// (e.g. `RunStarted`, `Warning`, `FileChanged`).
///
/// # Mapping rules
///
/// | `AgentEventKind`   | Gemini SSE shape                                    |
/// |--------------------|-----------------------------------------------------|
/// | `AssistantDelta`   | `candidates[0].content.parts[0].text`               |
/// | `ToolCall`         | `candidates[0].content.parts[0].functionCall`       |
/// | `RunCompleted`     | `candidates[0].finishReason = "STOP"`               |
#[must_use]
pub fn from_agent_event(event: &AgentEvent) -> Option<serde_json::Value> {
    match &event.kind {
        AgentEventKind::AssistantDelta { text } => Some(serde_json::json!({
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [{"text": text}]
                }
            }]
        })),
        AgentEventKind::ToolCall {
            tool_name, input, ..
        } => Some(serde_json::json!({
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [{
                        "functionCall": {
                            "name": tool_name,
                            "args": input
                        }
                    }]
                }
            }]
        })),
        AgentEventKind::RunCompleted { .. } => Some(serde_json::json!({
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": []
                },
                "finishReason": "STOP"
            }]
        })),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Generation config helpers
// ---------------------------------------------------------------------------

/// Extract sampling parameters from a Gemini [`GenerationConfig`] into a
/// flat map suitable for logging or diagnostics.
#[must_use]
pub fn generation_config_to_map(cfg: &GenerationConfig) -> BTreeMap<String, serde_json::Value> {
    let mut map = BTreeMap::new();
    if let Some(t) = cfg.temperature {
        map.insert("temperature".into(), serde_json::json!(t));
    }
    if let Some(p) = cfg.top_p {
        map.insert("top_p".into(), serde_json::json!(p));
    }
    if let Some(k) = cfg.top_k {
        map.insert("top_k".into(), serde_json::json!(k));
    }
    if let Some(m) = cfg.max_output_tokens {
        map.insert("max_output_tokens".into(), serde_json::json!(m));
    }
    if let Some(c) = cfg.candidate_count {
        map.insert("candidate_count".into(), serde_json::json!(c));
    }
    map
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        FunctionCallingConfig, FunctionCallingMode, FunctionDeclaration, GeminiTool,
        HarmBlockThreshold, HarmCategory, SafetySetting, ToolConfig,
    };
    use abp_core::{AgentEvent, AgentEventKind, Outcome, ReceiptBuilder, UsageNormalized};
    use chrono::Utc;

    // ── Helper factories ────────────────────────────────────────────────

    fn user_content(text: &str) -> Content {
        Content {
            role: Some("user".into()),
            parts: vec![Part::Text(text.into())],
        }
    }

    fn model_content(text: &str) -> Content {
        Content {
            role: Some("model".into()),
            parts: vec![Part::Text(text.into())],
        }
    }

    fn simple_request(text: &str) -> GenerateContentRequest {
        GenerateContentRequest {
            contents: vec![user_content(text)],
            system_instruction: None,
            tools: None,
            tool_config: None,
            generation_config: None,
            safety_settings: None,
        }
    }

    fn make_receipt(outcome: Outcome) -> Receipt {
        ReceiptBuilder::new("gemini").outcome(outcome).build()
    }

    fn make_wo() -> WorkOrder {
        WorkOrderBuilder::new("test").build()
    }

    // ── to_work_order tests ─────────────────────────────────────────────

    #[test]
    fn to_work_order_extracts_last_user_text() {
        let req = simple_request("Explain Rust traits");
        let wo = to_work_order(&req);
        assert_eq!(wo.task, "Explain Rust traits");
    }

    #[test]
    fn to_work_order_empty_contents() {
        let req = GenerateContentRequest {
            contents: vec![],
            system_instruction: None,
            tools: None,
            tool_config: None,
            generation_config: None,
            safety_settings: None,
        };
        let wo = to_work_order(&req);
        assert!(wo.task.is_empty());
    }

    #[test]
    fn to_work_order_multi_turn_extracts_last_user() {
        let req = GenerateContentRequest {
            contents: vec![
                user_content("First question"),
                model_content("Answer"),
                user_content("Follow-up"),
            ],
            system_instruction: None,
            tools: None,
            tool_config: None,
            generation_config: None,
            safety_settings: None,
        };
        let wo = to_work_order(&req);
        assert_eq!(wo.task, "Follow-up");
    }

    #[test]
    fn to_work_order_skips_non_text_parts_for_task() {
        let req = GenerateContentRequest {
            contents: vec![Content {
                role: Some("user".into()),
                parts: vec![Part::InlineData {
                    mime_type: "image/png".into(),
                    data: "base64".into(),
                }],
            }],
            system_instruction: None,
            tools: None,
            tool_config: None,
            generation_config: None,
            safety_settings: None,
        };
        let wo = to_work_order(&req);
        assert!(wo.task.is_empty());
    }

    #[test]
    fn to_work_order_with_system_instruction() {
        let req = GenerateContentRequest {
            contents: vec![user_content("Hello")],
            system_instruction: Some(Content {
                role: None,
                parts: vec![Part::Text("Be concise.".into())],
            }),
            tools: None,
            tool_config: None,
            generation_config: None,
            safety_settings: None,
        };
        let wo = to_work_order(&req);
        assert_eq!(wo.context.snippets.len(), 1);
        assert_eq!(wo.context.snippets[0].name, "system_instruction");
        assert_eq!(wo.context.snippets[0].content, "Be concise.");
    }

    #[test]
    fn to_work_order_multiple_system_parts() {
        let req = GenerateContentRequest {
            contents: vec![user_content("Hi")],
            system_instruction: Some(Content {
                role: None,
                parts: vec![
                    Part::Text("Rule one.".into()),
                    Part::Text("Rule two.".into()),
                ],
            }),
            tools: None,
            tool_config: None,
            generation_config: None,
            safety_settings: None,
        };
        let wo = to_work_order(&req);
        assert_eq!(wo.context.snippets.len(), 2);
        assert_eq!(wo.context.snippets[1].content, "Rule two.");
    }

    #[test]
    fn to_work_order_stores_dialect() {
        let wo = to_work_order(&simple_request("Hi"));
        assert_eq!(wo.config.vendor["dialect"], "gemini");
    }

    #[test]
    fn to_work_order_stores_contents() {
        let wo = to_work_order(&simple_request("Hi"));
        assert!(wo.config.vendor.contains_key("contents"));
    }

    #[test]
    fn to_work_order_preserves_tools_in_vendor() {
        let req = GenerateContentRequest {
            contents: vec![user_content("Search")],
            system_instruction: None,
            tools: Some(vec![GeminiTool {
                function_declarations: vec![FunctionDeclaration {
                    name: "search".into(),
                    description: "Search the web".into(),
                    parameters: serde_json::json!({"type": "object"}),
                }],
            }]),
            tool_config: None,
            generation_config: None,
            safety_settings: None,
        };
        let wo = to_work_order(&req);
        assert!(wo.config.vendor.contains_key("tools"));
    }

    #[test]
    fn to_work_order_preserves_generation_config() {
        let req = GenerateContentRequest {
            contents: vec![user_content("Hi")],
            system_instruction: None,
            tools: None,
            tool_config: None,
            generation_config: Some(GenerationConfig {
                temperature: Some(0.7),
                top_p: Some(0.9),
                top_k: Some(40),
                max_output_tokens: Some(1024),
                candidate_count: Some(1),
                stop_sequences: None,
            }),
            safety_settings: None,
        };
        let wo = to_work_order(&req);
        assert!(wo.config.vendor.contains_key("generation_config"));
        let gen_cfg = &wo.config.vendor["generation_config"];
        assert_eq!(gen_cfg["temperature"], 0.7);
        assert_eq!(gen_cfg["topP"], 0.9);
        assert_eq!(gen_cfg["topK"], 40);
        assert_eq!(gen_cfg["maxOutputTokens"], 1024);
        assert_eq!(gen_cfg["candidateCount"], 1);
    }

    #[test]
    fn to_work_order_preserves_safety_settings() {
        let req = GenerateContentRequest {
            contents: vec![user_content("Hi")],
            system_instruction: None,
            tools: None,
            tool_config: None,
            generation_config: None,
            safety_settings: Some(vec![SafetySetting {
                category: HarmCategory::HarmCategoryHarassment,
                threshold: HarmBlockThreshold::BlockNone,
            }]),
        };
        let wo = to_work_order(&req);
        assert!(wo.config.vendor.contains_key("safety_settings"));
    }

    #[test]
    fn to_work_order_preserves_tool_config() {
        let req = GenerateContentRequest {
            contents: vec![user_content("Hi")],
            system_instruction: None,
            tools: None,
            tool_config: Some(ToolConfig {
                function_calling_config: FunctionCallingConfig {
                    mode: FunctionCallingMode::Auto,
                    allowed_function_names: None,
                },
            }),
            generation_config: None,
            safety_settings: None,
        };
        let wo = to_work_order(&req);
        assert!(wo.config.vendor.contains_key("tool_config"));
    }

    #[test]
    fn to_work_order_model_is_none() {
        let wo = to_work_order(&simple_request("Hi"));
        assert!(wo.config.model.is_none());
    }

    // ── from_receipt tests ──────────────────────────────────────────────

    #[test]
    fn from_receipt_text() {
        let receipt = ReceiptBuilder::new("gemini")
            .outcome(Outcome::Complete)
            .add_trace_event(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: "Hello!".into(),
                },
                ext: None,
            })
            .build();
        let resp = from_receipt(&receipt, &make_wo());
        assert_eq!(resp.candidates.len(), 1);
        assert_eq!(resp.candidates[0].content.role.as_deref(), Some("model"));
        match &resp.candidates[0].content.parts[0] {
            Part::Text(t) => assert_eq!(t, "Hello!"),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn from_receipt_tool_call() {
        let receipt = ReceiptBuilder::new("gemini")
            .outcome(Outcome::Complete)
            .add_trace_event(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::ToolCall {
                    tool_name: "search".into(),
                    tool_use_id: None,
                    parent_tool_use_id: None,
                    input: serde_json::json!({"q": "rust"}),
                },
                ext: None,
            })
            .build();
        let resp = from_receipt(&receipt, &make_wo());
        match &resp.candidates[0].content.parts[0] {
            Part::FunctionCall { name, args } => {
                assert_eq!(name, "search");
                assert_eq!(args["q"], "rust");
            }
            other => panic!("expected FunctionCall, got {other:?}"),
        }
    }

    #[test]
    fn from_receipt_tool_result() {
        let receipt = ReceiptBuilder::new("gemini")
            .outcome(Outcome::Complete)
            .add_trace_event(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::ToolResult {
                    tool_name: "search".into(),
                    tool_use_id: None,
                    output: serde_json::json!({"results": []}),
                    is_error: false,
                },
                ext: None,
            })
            .build();
        let resp = from_receipt(&receipt, &make_wo());
        match &resp.candidates[0].content.parts[0] {
            Part::FunctionResponse { name, response } => {
                assert_eq!(name, "search");
                assert_eq!(response, &serde_json::json!({"results": []}));
            }
            other => panic!("expected FunctionResponse, got {other:?}"),
        }
    }

    #[test]
    fn from_receipt_complete_outcome() {
        let resp = from_receipt(&make_receipt(Outcome::Complete), &make_wo());
        assert_eq!(resp.candidates[0].finish_reason.as_deref(), Some("STOP"));
    }

    #[test]
    fn from_receipt_partial_outcome() {
        let resp = from_receipt(&make_receipt(Outcome::Partial), &make_wo());
        assert_eq!(
            resp.candidates[0].finish_reason.as_deref(),
            Some("MAX_TOKENS")
        );
    }

    #[test]
    fn from_receipt_failed_outcome() {
        let resp = from_receipt(&make_receipt(Outcome::Failed), &make_wo());
        assert_eq!(resp.candidates[0].finish_reason.as_deref(), Some("OTHER"));
    }

    #[test]
    fn from_receipt_with_usage() {
        let usage = UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            ..Default::default()
        };
        let receipt = ReceiptBuilder::new("gemini")
            .outcome(Outcome::Complete)
            .usage(usage)
            .build();
        let resp = from_receipt(&receipt, &make_wo());
        let meta = resp.usage_metadata.unwrap();
        assert_eq!(meta.prompt_token_count, 100);
        assert_eq!(meta.candidates_token_count, 50);
        assert_eq!(meta.total_token_count, 150);
    }

    #[test]
    fn from_receipt_no_usage() {
        let resp = from_receipt(&make_receipt(Outcome::Complete), &make_wo());
        assert!(resp.usage_metadata.is_none());
    }

    #[test]
    fn from_receipt_empty_trace() {
        let resp = from_receipt(&make_receipt(Outcome::Complete), &make_wo());
        assert!(resp.candidates[0].content.parts.is_empty());
    }

    #[test]
    fn from_receipt_no_prompt_feedback() {
        let resp = from_receipt(&make_receipt(Outcome::Complete), &make_wo());
        assert!(resp.prompt_feedback.is_none());
    }

    // ── from_agent_event tests ──────────────────────────────────────────

    #[test]
    fn from_agent_event_text_delta() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "Hello".into(),
            },
            ext: None,
        };
        let val = from_agent_event(&event).unwrap();
        assert_eq!(val["candidates"][0]["content"]["parts"][0]["text"], "Hello");
        assert_eq!(val["candidates"][0]["content"]["role"], "model");
    }

    #[test]
    fn from_agent_event_tool_call() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "search".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: serde_json::json!({"q": "rust"}),
            },
            ext: None,
        };
        let val = from_agent_event(&event).unwrap();
        let fc = &val["candidates"][0]["content"]["parts"][0]["functionCall"];
        assert_eq!(fc["name"], "search");
        assert_eq!(fc["args"]["q"], "rust");
    }

    #[test]
    fn from_agent_event_run_completed() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        };
        let val = from_agent_event(&event).unwrap();
        assert_eq!(val["candidates"][0]["finishReason"], "STOP");
    }

    #[test]
    fn from_agent_event_returns_none_for_run_started() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "start".into(),
            },
            ext: None,
        };
        assert!(from_agent_event(&event).is_none());
    }

    #[test]
    fn from_agent_event_returns_none_for_warning() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Warning {
                message: "warn".into(),
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
                path: "a.rs".into(),
                summary: "edited".into(),
            },
            ext: None,
        };
        assert!(from_agent_event(&event).is_none());
    }

    // ── Role mapping tests ──────────────────────────────────────────────

    #[test]
    fn gemini_role_model_to_canonical() {
        assert_eq!(gemini_role_to_canonical("model"), "assistant");
    }

    #[test]
    fn gemini_role_user_to_canonical() {
        assert_eq!(gemini_role_to_canonical("user"), "user");
    }

    #[test]
    fn gemini_role_unknown_defaults_to_user() {
        assert_eq!(gemini_role_to_canonical("narrator"), "user");
    }

    #[test]
    fn canonical_role_assistant_to_gemini() {
        assert_eq!(canonical_role_to_gemini("assistant"), "model");
    }

    #[test]
    fn canonical_role_user_to_gemini() {
        assert_eq!(canonical_role_to_gemini("user"), "user");
    }

    #[test]
    fn canonical_role_system_to_gemini() {
        assert_eq!(canonical_role_to_gemini("system"), "user");
    }

    // ── Part helper tests ───────────────────────────────────────────────

    #[test]
    fn part_text_extracts_text() {
        let p = Part::Text("hello".into());
        assert_eq!(part_text(&p), Some("hello"));
    }

    #[test]
    fn part_text_returns_none_for_non_text() {
        let p = Part::FunctionCall {
            name: "f".into(),
            args: serde_json::json!({}),
        };
        assert!(part_text(&p).is_none());
    }

    #[test]
    fn text_part_helper() {
        assert_eq!(text_part("hi"), Part::Text("hi".into()));
    }

    #[test]
    fn function_call_part_helper() {
        let p = function_call_part("f", serde_json::json!({"x": 1}));
        match p {
            Part::FunctionCall { name, args } => {
                assert_eq!(name, "f");
                assert_eq!(args, serde_json::json!({"x": 1}));
            }
            other => panic!("expected FunctionCall, got {other:?}"),
        }
    }

    #[test]
    fn function_response_part_helper() {
        let p = function_response_part("f", serde_json::json!("ok"));
        match p {
            Part::FunctionResponse { name, response } => {
                assert_eq!(name, "f");
                assert_eq!(response, serde_json::json!("ok"));
            }
            other => panic!("expected FunctionResponse, got {other:?}"),
        }
    }

    // ── generation_config_to_map tests ──────────────────────────────────

    #[test]
    fn generation_config_to_map_full() {
        let cfg = GenerationConfig {
            temperature: Some(0.5),
            top_p: Some(0.9),
            top_k: Some(40),
            max_output_tokens: Some(2048),
            candidate_count: Some(2),
            stop_sequences: None,
        };
        let map = generation_config_to_map(&cfg);
        assert_eq!(map["temperature"], 0.5);
        assert_eq!(map["top_p"], 0.9);
        assert_eq!(map["top_k"], 40);
        assert_eq!(map["max_output_tokens"], 2048);
        assert_eq!(map["candidate_count"], 2);
    }

    #[test]
    fn generation_config_to_map_empty() {
        let cfg = GenerationConfig::default();
        let map = generation_config_to_map(&cfg);
        assert!(map.is_empty());
    }

    // ── outcome_to_finish_reason tests ──────────────────────────────────

    #[test]
    fn outcome_complete_maps_to_stop() {
        assert_eq!(outcome_to_finish_reason(&Outcome::Complete), "STOP");
    }

    #[test]
    fn outcome_partial_maps_to_max_tokens() {
        assert_eq!(outcome_to_finish_reason(&Outcome::Partial), "MAX_TOKENS");
    }

    #[test]
    fn outcome_failed_maps_to_other() {
        assert_eq!(outcome_to_finish_reason(&Outcome::Failed), "OTHER");
    }

    // ── Roundtrip test ──────────────────────────────────────────────────

    #[test]
    fn roundtrip_request_to_receipt_to_response() {
        let req = simple_request("Hello");
        let wo = to_work_order(&req);
        assert_eq!(wo.task, "Hello");

        let receipt = ReceiptBuilder::new("gemini")
            .outcome(Outcome::Complete)
            .work_order_id(wo.id)
            .add_trace_event(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage { text: "Hi!".into() },
                ext: None,
            })
            .build();

        let resp = from_receipt(&receipt, &wo);
        assert_eq!(resp.candidates.len(), 1);
        assert_eq!(resp.candidates[0].finish_reason.as_deref(), Some("STOP"));
        match &resp.candidates[0].content.parts[0] {
            Part::Text(t) => assert_eq!(t, "Hi!"),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn part_to_json_produces_valid_json() {
        let p = Part::Text("hello".into());
        let v = part_to_json(&p);
        assert!(v.is_object() || v.is_string());
    }
}
