// SPDX-License-Identifier: MIT OR Apache-2.0
//! Conversion between Codex CLI wire-format types and ABP contract types.
//!
//! Codex extends the OpenAI chat-completions format with an `instructions`
//! field for system prompts and [`CodexFileChange`] / [`CodexCommand`] for
//! workspace mutations.  The helpers here bridge between those SDK-specific
//! types and the vendor-agnostic [`WorkOrder`] / [`Receipt`] contract.

use crate::types::{
    CodexChoice, CodexChoiceMessage, CodexFileChange, CodexMessage, CodexRequest, CodexResponse,
    CodexUsage, FileOperation,
};
use abp_core::{
    AgentEvent, AgentEventKind, Receipt, RuntimeConfig, WorkOrder, WorkOrderBuilder,
};
use abp_sdk_types::Dialect;
use std::collections::BTreeMap;

// ── Request → WorkOrder ─────────────────────────────────────────────────

/// Convert a [`CodexRequest`] into an ABP [`WorkOrder`].
///
/// The first user message becomes the work-order task.  If `instructions`
/// is set it is stored in `config.vendor["codex"]["instructions"]`.
/// Model and sampling parameters are forwarded into [`RuntimeConfig`].
pub fn to_work_order(req: &CodexRequest) -> WorkOrder {
    let task = extract_task(&req.messages);

    let mut vendor = BTreeMap::new();
    let mut codex_meta = serde_json::Map::new();

    if let Some(instr) = &req.instructions {
        codex_meta.insert("instructions".into(), serde_json::Value::String(instr.clone()));
    }

    codex_meta.insert(
        "dialect".into(),
        serde_json::to_value(Dialect::Codex).unwrap_or(serde_json::Value::String("codex".into())),
    );

    if let Some(temp) = req.temperature {
        codex_meta.insert(
            "temperature".into(),
            serde_json::Value::Number(serde_json::Number::from_f64(temp).unwrap_or_else(|| 0.into())),
        );
    }
    if let Some(top_p) = req.top_p {
        codex_meta.insert(
            "top_p".into(),
            serde_json::Value::Number(serde_json::Number::from_f64(top_p).unwrap_or_else(|| 0.into())),
        );
    }
    if let Some(stream) = req.stream {
        codex_meta.insert("stream".into(), serde_json::Value::Bool(stream));
    }

    vendor.insert("codex".into(), serde_json::Value::Object(codex_meta));

    let config = RuntimeConfig {
        model: Some(req.model.clone()),
        vendor,
        ..RuntimeConfig::default()
    };

    WorkOrderBuilder::new(task).config(config).build()
}

/// Extract the task string from Codex messages.
///
/// Concatenates all user message content; falls back to `"(empty)"`.
fn extract_task(messages: &[CodexMessage]) -> String {
    let parts: Vec<&str> = messages
        .iter()
        .filter_map(|m| match m {
            CodexMessage::User { content } => Some(content.as_str()),
            _ => None,
        })
        .collect();
    if parts.is_empty() {
        "(empty)".into()
    } else {
        parts.join("\n")
    }
}

// ── Receipt → Response ──────────────────────────────────────────────────

/// Convert an ABP [`Receipt`] (plus the originating [`WorkOrder`]) back
/// into a [`CodexResponse`].
///
/// Assistant messages are gathered from the receipt trace.  Token usage is
/// extracted from `receipt.usage`.
pub fn from_receipt(receipt: &Receipt, wo: &WorkOrder) -> CodexResponse {
    let mut content_parts: Vec<String> = Vec::new();

    for event in &receipt.trace {
        if let AgentEventKind::AssistantMessage { text } = &event.kind {
            content_parts.push(text.clone());
        }
    }

    let text = if content_parts.is_empty() {
        None
    } else {
        Some(content_parts.join("\n"))
    };

    let finish_reason = match receipt.outcome {
        abp_core::Outcome::Complete => Some("stop".into()),
        abp_core::Outcome::Partial => Some("length".into()),
        abp_core::Outcome::Failed => Some("stop".into()),
    };

    let usage = build_usage(receipt);

    CodexResponse {
        id: receipt.meta.run_id.to_string(),
        object: "chat.completion".into(),
        created: receipt.meta.started_at.timestamp() as u64,
        model: wo
            .config
            .model
            .clone()
            .unwrap_or_else(|| "codex-mini-latest".into()),
        choices: vec![CodexChoice {
            index: 0,
            message: CodexChoiceMessage {
                role: "assistant".into(),
                content: text,
                tool_calls: None,
            },
            finish_reason,
        }],
        usage,
    }
}

/// Build [`CodexUsage`] from a receipt's normalized usage counters.
fn build_usage(receipt: &Receipt) -> Option<CodexUsage> {
    let input = receipt.usage.input_tokens.unwrap_or(0);
    let output = receipt.usage.output_tokens.unwrap_or(0);
    if input == 0 && output == 0 {
        return None;
    }
    Some(CodexUsage {
        prompt_tokens: input,
        completion_tokens: output,
        total_tokens: input + output,
    })
}

// ── File-change helpers ─────────────────────────────────────────────────

/// Convert a [`CodexFileChange`] into an ABP [`AgentEvent`] with kind
/// [`AgentEventKind::FileChanged`].
pub fn file_change_to_event(fc: &CodexFileChange) -> AgentEvent {
    let summary = match fc.operation {
        FileOperation::Create => format!("Created {}", fc.path),
        FileOperation::Update => format!("Updated {}", fc.path),
        FileOperation::Delete => format!("Deleted {}", fc.path),
        FileOperation::Patch => format!("Patched {}", fc.path),
    };
    AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::FileChanged {
            path: fc.path.clone(),
            summary,
        },
        ext: None,
    }
}

/// Try to reconstruct a [`CodexFileChange`] from an ABP
/// [`AgentEventKind::FileChanged`] event.
///
/// Returns `None` if the event is not a `FileChanged` variant.
pub fn event_to_file_change(event: &AgentEvent) -> Option<CodexFileChange> {
    if let AgentEventKind::FileChanged { path, summary } = &event.kind {
        let operation = if summary.starts_with("Created") {
            FileOperation::Create
        } else if summary.starts_with("Deleted") {
            FileOperation::Delete
        } else if summary.starts_with("Patched") {
            FileOperation::Patch
        } else {
            FileOperation::Update
        };
        Some(CodexFileChange {
            path: path.clone(),
            operation,
            content: None,
            diff: None,
        })
    } else {
        None
    }
}

/// Collect all file-change events from a receipt trace as [`CodexFileChange`]s.
pub fn extract_file_changes(receipt: &Receipt) -> Vec<CodexFileChange> {
    receipt
        .trace
        .iter()
        .filter_map(event_to_file_change)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::{
        AgentEvent, AgentEventKind, Outcome, ReceiptBuilder, UsageNormalized, WorkOrderBuilder,
    };
    use chrono::Utc;

    fn sample_request() -> CodexRequest {
        CodexRequest {
            model: "codex-mini-latest".into(),
            messages: vec![CodexMessage::User {
                content: "Fix the bug".into(),
            }],
            instructions: Some("You are a coding assistant.".into()),
            temperature: Some(0.7),
            top_p: None,
            max_tokens: Some(4096),
            stream: None,
            tools: None,
            tool_choice: None,
        }
    }

    fn sample_receipt(wo: &WorkOrder) -> Receipt {
        ReceiptBuilder::new("codex")
            .work_order_id(wo.id)
            .outcome(Outcome::Complete)
            .usage(UsageNormalized {
                input_tokens: Some(100),
                output_tokens: Some(50),
                ..UsageNormalized::default()
            })
            .add_trace_event(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: "Bug fixed.".into(),
                },
                ext: None,
            })
            .build()
    }

    // ── to_work_order tests ──────────────────────────────────────────

    #[test]
    fn to_work_order_extracts_task_from_user_message() {
        let req = sample_request();
        let wo = to_work_order(&req);
        assert_eq!(wo.task, "Fix the bug");
    }

    #[test]
    fn to_work_order_sets_model() {
        let req = sample_request();
        let wo = to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("codex-mini-latest"));
    }

    #[test]
    fn to_work_order_stores_instructions_in_vendor() {
        let req = sample_request();
        let wo = to_work_order(&req);
        let codex = wo.config.vendor.get("codex").unwrap();
        assert_eq!(codex["instructions"], "You are a coding assistant.");
    }

    #[test]
    fn to_work_order_stores_dialect_in_vendor() {
        let req = sample_request();
        let wo = to_work_order(&req);
        let codex = wo.config.vendor.get("codex").unwrap();
        assert!(codex.get("dialect").is_some());
    }

    #[test]
    fn to_work_order_stores_temperature_in_vendor() {
        let req = sample_request();
        let wo = to_work_order(&req);
        let codex = wo.config.vendor.get("codex").unwrap();
        let temp = codex["temperature"].as_f64().unwrap();
        assert!((temp - 0.7).abs() < f64::EPSILON);
    }

    #[test]
    fn to_work_order_with_no_user_message_falls_back() {
        let req = CodexRequest {
            model: "codex-mini-latest".into(),
            messages: vec![CodexMessage::System {
                content: "system".into(),
            }],
            instructions: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
        };
        let wo = to_work_order(&req);
        assert_eq!(wo.task, "(empty)");
    }

    #[test]
    fn to_work_order_concatenates_multiple_user_messages() {
        let req = CodexRequest {
            model: "codex-mini-latest".into(),
            messages: vec![
                CodexMessage::User {
                    content: "Part 1".into(),
                },
                CodexMessage::User {
                    content: "Part 2".into(),
                },
            ],
            instructions: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
        };
        let wo = to_work_order(&req);
        assert!(wo.task.contains("Part 1"));
        assert!(wo.task.contains("Part 2"));
    }

    #[test]
    fn to_work_order_without_instructions_omits_key() {
        let mut req = sample_request();
        req.instructions = None;
        let wo = to_work_order(&req);
        let codex = wo.config.vendor.get("codex").unwrap();
        assert!(codex.get("instructions").is_none());
    }

    // ── from_receipt tests ───────────────────────────────────────────

    #[test]
    fn from_receipt_produces_valid_response() {
        let wo = WorkOrderBuilder::new("task").model("codex-mini-latest").build();
        let receipt = sample_receipt(&wo);
        let resp = from_receipt(&receipt, &wo);
        assert_eq!(resp.object, "chat.completion");
        assert_eq!(resp.model, "codex-mini-latest");
    }

    #[test]
    fn from_receipt_includes_assistant_text() {
        let wo = WorkOrderBuilder::new("task").model("codex-mini-latest").build();
        let receipt = sample_receipt(&wo);
        let resp = from_receipt(&receipt, &wo);
        let text = resp.choices[0].message.content.as_deref().unwrap();
        assert_eq!(text, "Bug fixed.");
    }

    #[test]
    fn from_receipt_sets_finish_reason_stop_on_complete() {
        let wo = WorkOrderBuilder::new("task").build();
        let receipt = sample_receipt(&wo);
        let resp = from_receipt(&receipt, &wo);
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn from_receipt_sets_finish_reason_length_on_partial() {
        let wo = WorkOrderBuilder::new("task").build();
        let receipt = ReceiptBuilder::new("codex")
            .outcome(Outcome::Partial)
            .build();
        let resp = from_receipt(&receipt, &wo);
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("length"));
    }

    #[test]
    fn from_receipt_includes_usage() {
        let wo = WorkOrderBuilder::new("task").build();
        let receipt = sample_receipt(&wo);
        let resp = from_receipt(&receipt, &wo);
        let usage = resp.usage.as_ref().unwrap();
        assert_eq!(usage.prompt_tokens, 100);
        assert_eq!(usage.completion_tokens, 50);
        assert_eq!(usage.total_tokens, 150);
    }

    #[test]
    fn from_receipt_no_usage_when_zero() {
        let wo = WorkOrderBuilder::new("task").build();
        let receipt = ReceiptBuilder::new("codex")
            .outcome(Outcome::Complete)
            .build();
        let resp = from_receipt(&receipt, &wo);
        assert!(resp.usage.is_none());
    }

    #[test]
    fn from_receipt_uses_run_id_as_response_id() {
        let wo = WorkOrderBuilder::new("task").build();
        let receipt = sample_receipt(&wo);
        let resp = from_receipt(&receipt, &wo);
        assert_eq!(resp.id, receipt.meta.run_id.to_string());
    }

    // ── File-change helpers ──────────────────────────────────────────

    #[test]
    fn file_change_to_event_create() {
        let fc = CodexFileChange {
            path: "src/main.rs".into(),
            operation: FileOperation::Create,
            content: Some("fn main() {}".into()),
            diff: None,
        };
        let event = file_change_to_event(&fc);
        match &event.kind {
            AgentEventKind::FileChanged { path, summary } => {
                assert_eq!(path, "src/main.rs");
                assert!(summary.contains("Created"));
            }
            other => panic!("expected FileChanged, got {other:?}"),
        }
    }

    #[test]
    fn file_change_to_event_delete() {
        let fc = CodexFileChange {
            path: "old.txt".into(),
            operation: FileOperation::Delete,
            content: None,
            diff: None,
        };
        let event = file_change_to_event(&fc);
        if let AgentEventKind::FileChanged { summary, .. } = &event.kind {
            assert!(summary.contains("Deleted"));
        }
    }

    #[test]
    fn event_to_file_change_roundtrip() {
        let fc = CodexFileChange {
            path: "lib.rs".into(),
            operation: FileOperation::Update,
            content: None,
            diff: None,
        };
        let event = file_change_to_event(&fc);
        let reconstructed = event_to_file_change(&event).unwrap();
        assert_eq!(reconstructed.path, "lib.rs");
        assert_eq!(reconstructed.operation, FileOperation::Update);
    }

    #[test]
    fn event_to_file_change_returns_none_for_non_file_event() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "hello".into(),
            },
            ext: None,
        };
        assert!(event_to_file_change(&event).is_none());
    }

    #[test]
    fn extract_file_changes_from_receipt() {
        let wo = WorkOrderBuilder::new("task").build();
        let receipt = ReceiptBuilder::new("codex")
            .outcome(Outcome::Complete)
            .add_trace_event(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::FileChanged {
                    path: "a.rs".into(),
                    summary: "Created a.rs".into(),
                },
                ext: None,
            })
            .add_trace_event(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::FileChanged {
                    path: "b.rs".into(),
                    summary: "Updated b.rs".into(),
                },
                ext: None,
            })
            .build();
        let changes = extract_file_changes(&receipt);
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].path, "a.rs");
        assert_eq!(changes[0].operation, FileOperation::Create);
        assert_eq!(changes[1].path, "b.rs");
        assert_eq!(changes[1].operation, FileOperation::Update);
        // suppress unused variable warning
        let _ = wo;
    }
}
