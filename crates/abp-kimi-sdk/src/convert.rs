// SPDX-License-Identifier: MIT OR Apache-2.0
//! Conversion between Kimi (Moonshot) wire-format types and ABP contract types.
//!
//! Kimi extends the OpenAI chat-completions format with `use_search` and
//! `SearchOptions` for built-in web search.  These helpers bridge between
//! those SDK-specific types and the vendor-agnostic `WorkOrder` / `Receipt`.

use crate::types::{
    ChatMessage, Choice, ChoiceMessage, KimiChatRequest, KimiChatResponse, KimiUsage, SearchOptions,
};
use abp_core::{AgentEventKind, Receipt, RuntimeConfig, WorkOrder, WorkOrderBuilder};
use abp_sdk_types::Dialect;
use std::collections::BTreeMap;

// ── Request → WorkOrder ─────────────────────────────────────────────────

/// Convert a [`KimiChatRequest`] into an ABP [`WorkOrder`].
///
/// The first user message becomes the work-order task.  Search-related
/// fields (`use_search`, `search_options`) are stored under
/// `config.vendor["kimi"]`.
pub fn to_work_order(req: &KimiChatRequest) -> WorkOrder {
    let task = extract_task(&req.messages);

    let mut vendor = BTreeMap::new();
    let mut kimi_meta = serde_json::Map::new();

    kimi_meta.insert(
        "dialect".into(),
        serde_json::to_value(Dialect::Kimi).unwrap_or(serde_json::Value::String("kimi".into())),
    );

    if let Some(use_search) = req.use_search {
        kimi_meta.insert("use_search".into(), serde_json::Value::Bool(use_search));
    }

    if let Some(opts) = &req.search_options {
        if let Ok(v) = serde_json::to_value(opts) {
            kimi_meta.insert("search_options".into(), v);
        }
    }

    if let Some(temp) = req.temperature {
        kimi_meta.insert(
            "temperature".into(),
            serde_json::Value::Number(
                serde_json::Number::from_f64(temp).unwrap_or_else(|| 0.into()),
            ),
        );
    }
    if let Some(top_p) = req.top_p {
        kimi_meta.insert(
            "top_p".into(),
            serde_json::Value::Number(
                serde_json::Number::from_f64(top_p).unwrap_or_else(|| 0.into()),
            ),
        );
    }
    if let Some(stream) = req.stream {
        kimi_meta.insert("stream".into(), serde_json::Value::Bool(stream));
    }

    vendor.insert("kimi".into(), serde_json::Value::Object(kimi_meta));

    let config = RuntimeConfig {
        model: Some(req.model.clone()),
        vendor,
        ..RuntimeConfig::default()
    };

    WorkOrderBuilder::new(task).config(config).build()
}

/// Extract the task string from Kimi chat messages.
///
/// Concatenates all user message content; falls back to `"(empty)"`.
fn extract_task(messages: &[ChatMessage]) -> String {
    let parts: Vec<&str> = messages
        .iter()
        .filter_map(|m| match m {
            ChatMessage::User { content } => Some(content.as_str()),
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
/// into a [`KimiChatResponse`].
///
/// Assistant messages are gathered from the receipt trace.  Token usage is
/// extracted from `receipt.usage`.
pub fn from_receipt(receipt: &Receipt, wo: &WorkOrder) -> KimiChatResponse {
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

    KimiChatResponse {
        id: receipt.meta.run_id.to_string(),
        object: "chat.completion".into(),
        created: receipt.meta.started_at.timestamp() as u64,
        model: wo
            .config
            .model
            .clone()
            .unwrap_or_else(|| "moonshot-v1-8k".into()),
        choices: vec![Choice {
            index: 0,
            message: ChoiceMessage {
                role: "assistant".into(),
                content: text,
                tool_calls: None,
            },
            finish_reason,
        }],
        usage,
    }
}

/// Build [`KimiUsage`] from a receipt's normalized usage counters.
fn build_usage(receipt: &Receipt) -> Option<KimiUsage> {
    let input = receipt.usage.input_tokens.unwrap_or(0);
    let output = receipt.usage.output_tokens.unwrap_or(0);
    if input == 0 && output == 0 {
        return None;
    }
    Some(KimiUsage {
        prompt_tokens: input,
        completion_tokens: output,
        total_tokens: input + output,
        search_tokens: None,
    })
}

// ── Search metadata helpers ─────────────────────────────────────────────

/// Extract search-related metadata from a work order's vendor config.
///
/// Returns `(use_search, search_options)` parsed from `vendor["kimi"]`.
pub fn extract_search_metadata(wo: &WorkOrder) -> (Option<bool>, Option<SearchOptions>) {
    let kimi = match wo.config.vendor.get("kimi") {
        Some(v) => v,
        None => return (None, None),
    };

    let use_search = kimi.get("use_search").and_then(|v| v.as_bool());

    let search_options = kimi
        .get("search_options")
        .and_then(|v| serde_json::from_value::<SearchOptions>(v.clone()).ok());

    (use_search, search_options)
}

/// Store search metadata into a vendor config map suitable for
/// inclusion in a [`RuntimeConfig`].
pub fn build_search_vendor_config(
    use_search: Option<bool>,
    search_options: Option<&SearchOptions>,
) -> BTreeMap<String, serde_json::Value> {
    let mut vendor = BTreeMap::new();
    let mut kimi_meta = serde_json::Map::new();

    if let Some(v) = use_search {
        kimi_meta.insert("use_search".into(), serde_json::Value::Bool(v));
    }
    if let Some(opts) = search_options {
        if let Ok(v) = serde_json::to_value(opts) {
            kimi_meta.insert("search_options".into(), v);
        }
    }

    if !kimi_meta.is_empty() {
        vendor.insert("kimi".into(), serde_json::Value::Object(kimi_meta));
    }
    vendor
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SearchMode;
    use abp_core::{
        AgentEvent, AgentEventKind, Outcome, ReceiptBuilder, UsageNormalized, WorkOrderBuilder,
    };
    use chrono::Utc;

    fn sample_request() -> KimiChatRequest {
        KimiChatRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![ChatMessage::User {
                content: "What is Rust?".into(),
            }],
            temperature: Some(0.5),
            top_p: None,
            max_tokens: Some(4096),
            stream: None,
            tools: None,
            tool_choice: None,
            use_search: Some(true),
            search_options: Some(SearchOptions {
                mode: SearchMode::Auto,
                result_count: Some(5),
            }),
        }
    }

    fn sample_receipt(wo: &WorkOrder) -> Receipt {
        ReceiptBuilder::new("kimi")
            .work_order_id(wo.id)
            .outcome(Outcome::Complete)
            .usage(UsageNormalized {
                input_tokens: Some(200),
                output_tokens: Some(80),
                ..UsageNormalized::default()
            })
            .add_trace_event(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: "Rust is a systems programming language.".into(),
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
        assert_eq!(wo.task, "What is Rust?");
    }

    #[test]
    fn to_work_order_sets_model() {
        let req = sample_request();
        let wo = to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-8k"));
    }

    #[test]
    fn to_work_order_stores_use_search() {
        let req = sample_request();
        let wo = to_work_order(&req);
        let kimi = wo.config.vendor.get("kimi").unwrap();
        assert_eq!(kimi["use_search"], true);
    }

    #[test]
    fn to_work_order_stores_search_options() {
        let req = sample_request();
        let wo = to_work_order(&req);
        let kimi = wo.config.vendor.get("kimi").unwrap();
        assert!(kimi.get("search_options").is_some());
        let opts = kimi["search_options"].clone();
        assert_eq!(opts["result_count"], 5);
    }

    #[test]
    fn to_work_order_stores_dialect() {
        let req = sample_request();
        let wo = to_work_order(&req);
        let kimi = wo.config.vendor.get("kimi").unwrap();
        assert!(kimi.get("dialect").is_some());
    }

    #[test]
    fn to_work_order_stores_temperature() {
        let req = sample_request();
        let wo = to_work_order(&req);
        let kimi = wo.config.vendor.get("kimi").unwrap();
        let temp = kimi["temperature"].as_f64().unwrap();
        assert!((temp - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn to_work_order_no_user_message_falls_back() {
        let req = KimiChatRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![ChatMessage::System {
                content: "system".into(),
            }],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
            use_search: None,
            search_options: None,
        };
        let wo = to_work_order(&req);
        assert_eq!(wo.task, "(empty)");
    }

    #[test]
    fn to_work_order_without_search_omits_keys() {
        let mut req = sample_request();
        req.use_search = None;
        req.search_options = None;
        let wo = to_work_order(&req);
        let kimi = wo.config.vendor.get("kimi").unwrap();
        assert!(kimi.get("use_search").is_none());
        assert!(kimi.get("search_options").is_none());
    }

    // ── from_receipt tests ───────────────────────────────────────────

    #[test]
    fn from_receipt_produces_valid_response() {
        let wo = WorkOrderBuilder::new("task")
            .model("moonshot-v1-8k")
            .build();
        let receipt = sample_receipt(&wo);
        let resp = from_receipt(&receipt, &wo);
        assert_eq!(resp.object, "chat.completion");
        assert_eq!(resp.model, "moonshot-v1-8k");
    }

    #[test]
    fn from_receipt_includes_assistant_text() {
        let wo = WorkOrderBuilder::new("task")
            .model("moonshot-v1-8k")
            .build();
        let receipt = sample_receipt(&wo);
        let resp = from_receipt(&receipt, &wo);
        let text = resp.choices[0].message.content.as_deref().unwrap();
        assert!(text.contains("Rust is a systems programming language"));
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
        let receipt = ReceiptBuilder::new("kimi")
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
        assert_eq!(usage.prompt_tokens, 200);
        assert_eq!(usage.completion_tokens, 80);
        assert_eq!(usage.total_tokens, 280);
        assert!(usage.search_tokens.is_none());
    }

    #[test]
    fn from_receipt_no_usage_when_zero() {
        let wo = WorkOrderBuilder::new("task").build();
        let receipt = ReceiptBuilder::new("kimi")
            .outcome(Outcome::Complete)
            .build();
        let resp = from_receipt(&receipt, &wo);
        assert!(resp.usage.is_none());
    }

    // ── Search metadata helpers ──────────────────────────────────────

    #[test]
    fn extract_search_metadata_from_work_order() {
        let req = sample_request();
        let wo = to_work_order(&req);
        let (use_search, opts) = extract_search_metadata(&wo);
        assert_eq!(use_search, Some(true));
        assert!(opts.is_some());
        assert_eq!(opts.unwrap().result_count, Some(5));
    }

    #[test]
    fn extract_search_metadata_returns_none_when_absent() {
        let wo = WorkOrderBuilder::new("task").build();
        let (use_search, opts) = extract_search_metadata(&wo);
        assert!(use_search.is_none());
        assert!(opts.is_none());
    }

    #[test]
    fn build_search_vendor_config_roundtrip() {
        let opts = SearchOptions {
            mode: SearchMode::Always,
            result_count: Some(10),
        };
        let vendor = build_search_vendor_config(Some(true), Some(&opts));
        let kimi = vendor.get("kimi").unwrap();
        assert_eq!(kimi["use_search"], true);
        let decoded: SearchOptions =
            serde_json::from_value(kimi["search_options"].clone()).unwrap();
        assert_eq!(decoded.mode, SearchMode::Always);
        assert_eq!(decoded.result_count, Some(10));
    }
}
