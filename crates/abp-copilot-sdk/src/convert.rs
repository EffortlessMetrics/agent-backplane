// SPDX-License-Identifier: MIT OR Apache-2.0
//! Conversion between Copilot wire-format types and ABP contract types.
//!
//! Copilot extends the OpenAI chat-completions format with `intent` and
//! `Reference` types (file, selection, terminal, web page, git diff).
//! These helpers bridge between those SDK-specific types and the
//! vendor-agnostic `WorkOrder` / `Receipt`.

use crate::types::{
    CopilotChatChoice, CopilotChatChoiceMessage, CopilotChatMessage, CopilotChatRequest,
    CopilotChatResponse, CopilotUsage, Reference, ReferenceType,
};
use abp_core::{
    AgentEventKind, ContextSnippet, Receipt, RuntimeConfig, WorkOrder, WorkOrderBuilder,
};
use abp_sdk_types::Dialect;
use std::collections::BTreeMap;

// ── Request → WorkOrder ─────────────────────────────────────────────────

/// Convert a [`CopilotChatRequest`] into an ABP [`WorkOrder`].
///
/// The first user message becomes the work-order task.  `intent` and
/// `references` are stored under `config.vendor["copilot"]`.
/// File and selection references are also mapped into the work-order
/// context packet.
pub fn to_work_order(req: &CopilotChatRequest) -> WorkOrder {
    let task = extract_task(&req.messages);

    let mut vendor = BTreeMap::new();
    let mut copilot_meta = serde_json::Map::new();

    copilot_meta.insert(
        "dialect".into(),
        serde_json::to_value(Dialect::Copilot)
            .unwrap_or(serde_json::Value::String("copilot".into())),
    );

    if let Some(intent) = &req.intent {
        copilot_meta.insert("intent".into(), serde_json::Value::String(intent.clone()));
    }

    if let Some(refs) = &req.references {
        if let Ok(v) = serde_json::to_value(refs) {
            copilot_meta.insert("references".into(), v);
        }
    }

    if let Some(temp) = req.temperature {
        copilot_meta.insert(
            "temperature".into(),
            serde_json::Value::Number(
                serde_json::Number::from_f64(temp).unwrap_or_else(|| 0.into()),
            ),
        );
    }
    if let Some(top_p) = req.top_p {
        copilot_meta.insert(
            "top_p".into(),
            serde_json::Value::Number(
                serde_json::Number::from_f64(top_p).unwrap_or_else(|| 0.into()),
            ),
        );
    }
    if let Some(stream) = req.stream {
        copilot_meta.insert("stream".into(), serde_json::Value::Bool(stream));
    }

    vendor.insert("copilot".into(), serde_json::Value::Object(copilot_meta));

    let config = RuntimeConfig {
        model: Some(req.model.clone()),
        vendor,
        ..RuntimeConfig::default()
    };

    // Map references into context
    let mut context_files = Vec::new();
    let mut context_snippets = Vec::new();

    if let Some(refs) = &req.references {
        for r in refs {
            match r.ref_type {
                ReferenceType::File => {
                    if let Some(uri) = &r.uri {
                        context_files.push(uri.clone());
                    }
                }
                ReferenceType::Selection => {
                    if let Some(content) = &r.content {
                        context_snippets.push(ContextSnippet {
                            name: format!("selection:{}", r.id),
                            content: content.clone(),
                        });
                    }
                }
                _ => {}
            }
        }
    }

    let context = abp_core::ContextPacket {
        files: context_files,
        snippets: context_snippets,
    };

    WorkOrderBuilder::new(task)
        .config(config)
        .context(context)
        .build()
}

/// Extract the task string from Copilot chat messages.
///
/// Concatenates all user message content; falls back to `"(empty)"`.
fn extract_task(messages: &[CopilotChatMessage]) -> String {
    let parts: Vec<&str> = messages
        .iter()
        .filter_map(|m| {
            if m.role == "user" {
                m.content.as_deref()
            } else {
                None
            }
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
/// into a [`CopilotChatResponse`].
///
/// Assistant messages are gathered from the receipt trace.  Token usage is
/// extracted from `receipt.usage`.
pub fn from_receipt(receipt: &Receipt, wo: &WorkOrder) -> CopilotChatResponse {
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

    CopilotChatResponse {
        id: receipt.meta.run_id.to_string(),
        object: "chat.completion".into(),
        created: receipt.meta.started_at.timestamp() as u64,
        model: wo.config.model.clone().unwrap_or_else(|| "gpt-4o".into()),
        choices: vec![CopilotChatChoice {
            index: 0,
            message: CopilotChatChoiceMessage {
                role: "assistant".into(),
                content: text,
                tool_calls: None,
            },
            finish_reason,
        }],
        usage,
    }
}

/// Build [`CopilotUsage`] from a receipt's normalized usage counters.
fn build_usage(receipt: &Receipt) -> Option<CopilotUsage> {
    let input = receipt.usage.input_tokens.unwrap_or(0);
    let output = receipt.usage.output_tokens.unwrap_or(0);
    if input == 0 && output == 0 {
        return None;
    }
    Some(CopilotUsage {
        prompt_tokens: input,
        completion_tokens: output,
        total_tokens: input + output,
        copilot_tokens: None,
    })
}

// ── Reference helpers ───────────────────────────────────────────────────

/// Create a file [`Reference`].
pub fn file_reference(id: impl Into<String>, uri: impl Into<String>) -> Reference {
    Reference {
        ref_type: ReferenceType::File,
        id: id.into(),
        uri: Some(uri.into()),
        content: None,
        metadata: None,
    }
}

/// Create a selection [`Reference`] with inline content.
pub fn selection_reference(id: impl Into<String>, content: impl Into<String>) -> Reference {
    Reference {
        ref_type: ReferenceType::Selection,
        id: id.into(),
        uri: None,
        content: Some(content.into()),
        metadata: None,
    }
}

/// Create a repository [`Reference`] stored via metadata.
pub fn repo_reference(
    id: impl Into<String>,
    metadata: BTreeMap<String, serde_json::Value>,
) -> Reference {
    Reference {
        ref_type: ReferenceType::Terminal,
        id: id.into(),
        uri: None,
        content: None,
        metadata: Some(metadata),
    }
}

/// Create a git-diff [`Reference`] with diff content.
pub fn git_diff_reference(id: impl Into<String>, diff: impl Into<String>) -> Reference {
    Reference {
        ref_type: ReferenceType::GitDiff,
        id: id.into(),
        uri: None,
        content: Some(diff.into()),
        metadata: None,
    }
}

/// Extract references of a specific type from a slice.
pub fn filter_references(refs: &[Reference], ref_type: ReferenceType) -> Vec<&Reference> {
    refs.iter().filter(|r| r.ref_type == ref_type).collect()
}

/// Extract Copilot references stored in a work order's vendor config.
///
/// Returns `None` if no references are stored.
pub fn extract_references(wo: &WorkOrder) -> Option<Vec<Reference>> {
    let copilot = wo.config.vendor.get("copilot")?;
    let refs_val = copilot.get("references")?;
    serde_json::from_value(refs_val.clone()).ok()
}

/// Extract the Copilot intent stored in a work order's vendor config.
pub fn extract_intent(wo: &WorkOrder) -> Option<String> {
    let copilot = wo.config.vendor.get("copilot")?;
    copilot.get("intent")?.as_str().map(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::{
        AgentEvent, AgentEventKind, Outcome, ReceiptBuilder, UsageNormalized, WorkOrderBuilder,
    };
    use chrono::Utc;

    fn sample_request() -> CopilotChatRequest {
        CopilotChatRequest {
            model: "gpt-4o".into(),
            messages: vec![CopilotChatMessage {
                role: "user".into(),
                content: Some("Review my code".into()),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            temperature: Some(0.3),
            top_p: None,
            max_tokens: Some(4096),
            stream: None,
            tools: None,
            tool_choice: None,
            intent: Some("code-review".into()),
            references: Some(vec![
                file_reference("f1", "file:///src/main.rs"),
                selection_reference("s1", "fn main() {}"),
                git_diff_reference("d1", "--- a/x\n+++ b/x\n@@ -1 +1 @@\n-old\n+new"),
            ]),
        }
    }

    fn sample_receipt(wo: &WorkOrder) -> Receipt {
        ReceiptBuilder::new("copilot")
            .work_order_id(wo.id)
            .outcome(Outcome::Complete)
            .usage(UsageNormalized {
                input_tokens: Some(300),
                output_tokens: Some(120),
                ..UsageNormalized::default()
            })
            .add_trace_event(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: "Code looks good!".into(),
                },
                ext: None,
            })
            .build()
    }

    // ── to_work_order tests ──────────────────────────────────────────

    #[test]
    fn to_work_order_extracts_task() {
        let req = sample_request();
        let wo = to_work_order(&req);
        assert_eq!(wo.task, "Review my code");
    }

    #[test]
    fn to_work_order_sets_model() {
        let req = sample_request();
        let wo = to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
    }

    #[test]
    fn to_work_order_stores_intent() {
        let req = sample_request();
        let wo = to_work_order(&req);
        let copilot = wo.config.vendor.get("copilot").unwrap();
        assert_eq!(copilot["intent"], "code-review");
    }

    #[test]
    fn to_work_order_stores_references() {
        let req = sample_request();
        let wo = to_work_order(&req);
        let copilot = wo.config.vendor.get("copilot").unwrap();
        let refs = copilot["references"].as_array().unwrap();
        assert_eq!(refs.len(), 3);
    }

    #[test]
    fn to_work_order_stores_dialect() {
        let req = sample_request();
        let wo = to_work_order(&req);
        let copilot = wo.config.vendor.get("copilot").unwrap();
        assert!(copilot.get("dialect").is_some());
    }

    #[test]
    fn to_work_order_maps_file_refs_to_context() {
        let req = sample_request();
        let wo = to_work_order(&req);
        assert!(
            wo.context
                .files
                .contains(&"file:///src/main.rs".to_string())
        );
    }

    #[test]
    fn to_work_order_maps_selection_refs_to_snippets() {
        let req = sample_request();
        let wo = to_work_order(&req);
        assert_eq!(wo.context.snippets.len(), 1);
        assert!(wo.context.snippets[0].content.contains("fn main()"));
    }

    #[test]
    fn to_work_order_no_user_message_falls_back() {
        let req = CopilotChatRequest {
            model: "gpt-4o".into(),
            messages: vec![CopilotChatMessage {
                role: "system".into(),
                content: Some("system".into()),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
            intent: None,
            references: None,
        };
        let wo = to_work_order(&req);
        assert_eq!(wo.task, "(empty)");
    }

    #[test]
    fn to_work_order_without_intent_omits_key() {
        let mut req = sample_request();
        req.intent = None;
        let wo = to_work_order(&req);
        let copilot = wo.config.vendor.get("copilot").unwrap();
        assert!(copilot.get("intent").is_none());
    }

    // ── from_receipt tests ───────────────────────────────────────────

    #[test]
    fn from_receipt_produces_valid_response() {
        let wo = WorkOrderBuilder::new("task").model("gpt-4o").build();
        let receipt = sample_receipt(&wo);
        let resp = from_receipt(&receipt, &wo);
        assert_eq!(resp.object, "chat.completion");
        assert_eq!(resp.model, "gpt-4o");
    }

    #[test]
    fn from_receipt_includes_assistant_text() {
        let wo = WorkOrderBuilder::new("task").model("gpt-4o").build();
        let receipt = sample_receipt(&wo);
        let resp = from_receipt(&receipt, &wo);
        let text = resp.choices[0].message.content.as_deref().unwrap();
        assert_eq!(text, "Code looks good!");
    }

    #[test]
    fn from_receipt_sets_stop_on_complete() {
        let wo = WorkOrderBuilder::new("task").build();
        let receipt = sample_receipt(&wo);
        let resp = from_receipt(&receipt, &wo);
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn from_receipt_sets_length_on_partial() {
        let wo = WorkOrderBuilder::new("task").build();
        let receipt = ReceiptBuilder::new("copilot")
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
        assert_eq!(usage.prompt_tokens, 300);
        assert_eq!(usage.completion_tokens, 120);
        assert_eq!(usage.total_tokens, 420);
        assert!(usage.copilot_tokens.is_none());
    }

    #[test]
    fn from_receipt_no_usage_when_zero() {
        let wo = WorkOrderBuilder::new("task").build();
        let receipt = ReceiptBuilder::new("copilot")
            .outcome(Outcome::Complete)
            .build();
        let resp = from_receipt(&receipt, &wo);
        assert!(resp.usage.is_none());
    }

    // ── Reference helpers ────────────────────────────────────────────

    #[test]
    fn file_reference_has_correct_type() {
        let r = file_reference("f1", "file:///foo.rs");
        assert_eq!(r.ref_type, ReferenceType::File);
        assert_eq!(r.uri.as_deref(), Some("file:///foo.rs"));
    }

    #[test]
    fn selection_reference_has_content() {
        let r = selection_reference("s1", "selected text");
        assert_eq!(r.ref_type, ReferenceType::Selection);
        assert_eq!(r.content.as_deref(), Some("selected text"));
    }

    #[test]
    fn git_diff_reference_has_content() {
        let r = git_diff_reference("d1", "+new line");
        assert_eq!(r.ref_type, ReferenceType::GitDiff);
        assert!(r.content.as_deref().unwrap().contains("+new line"));
    }

    #[test]
    fn filter_references_by_type() {
        let refs = vec![
            file_reference("f1", "a.rs"),
            selection_reference("s1", "x"),
            file_reference("f2", "b.rs"),
        ];
        let files = filter_references(&refs, ReferenceType::File);
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn extract_references_from_work_order() {
        let req = sample_request();
        let wo = to_work_order(&req);
        let refs = extract_references(&wo).unwrap();
        assert_eq!(refs.len(), 3);
    }

    #[test]
    fn extract_intent_from_work_order() {
        let req = sample_request();
        let wo = to_work_order(&req);
        let intent = extract_intent(&wo).unwrap();
        assert_eq!(intent, "code-review");
    }

    #[test]
    fn extract_intent_returns_none_when_absent() {
        let wo = WorkOrderBuilder::new("task").build();
        assert!(extract_intent(&wo).is_none());
    }
}
