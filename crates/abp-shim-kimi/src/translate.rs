// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code, unused_imports)]
//! Translation functions between Kimi-specific shim types and ABP core types.
//!
//! These functions bridge the Kimi Chat Completions API types (with extensions
//! like `use_search`, `ref_file_ids`, `plugin_ids`) to ABP's [`WorkOrder`],
//! [`Receipt`], and [`AgentEvent`] types.

use std::collections::BTreeMap;

use abp_core::{
    AgentEvent, AgentEventKind, Receipt, RuntimeConfig, UsageNormalized, WorkOrder,
    WorkOrderBuilder,
};
use abp_kimi_sdk::dialect::{KimiFunctionCall, KimiToolCall};
use chrono::Utc;

use crate::types::{
    KimiChatChoice, KimiChatChoiceMessage, KimiChatRequest, KimiChatResponse, KimiSearchResult,
    KimiStreamChoice, KimiStreamDelta, KimiStreamEvent, Message, Usage,
};

// ── KimiChatRequest → WorkOrder ─────────────────────────────────────────

/// Convert a [`KimiChatRequest`] into an ABP [`WorkOrder`].
///
/// Maps:
/// - Last user message → `work_order.task`
/// - `model` → `work_order.config.model`
/// - `temperature`, `max_tokens`, `top_p` → `work_order.config.vendor`
/// - `use_search`, `ref_file_ids`, `plugin_ids` → `work_order.config.vendor` (under `kimi.*` keys)
pub fn kimi_to_work_order(request: &KimiChatRequest) -> WorkOrder {
    let task = request
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .and_then(|m| m.content.clone())
        .unwrap_or_else(|| "kimi completion".into());

    let mut vendor = BTreeMap::new();

    if let Some(temp) = request.temperature {
        vendor.insert("temperature".to_string(), serde_json::Value::from(temp));
    }
    if let Some(max) = request.max_tokens {
        vendor.insert("max_tokens".to_string(), serde_json::Value::from(max));
    }
    if let Some(top_p) = request.top_p {
        vendor.insert("top_p".to_string(), serde_json::Value::from(top_p));
    }
    if let Some(use_search) = request.use_search {
        vendor.insert(
            "kimi.use_search".to_string(),
            serde_json::Value::from(use_search),
        );
    }
    if let Some(ref_file_ids) = &request.ref_file_ids {
        vendor.insert(
            "kimi.ref_file_ids".to_string(),
            serde_json::to_value(ref_file_ids).unwrap_or(serde_json::Value::Null),
        );
    }
    if let Some(plugin_ids) = &request.plugin_ids {
        vendor.insert(
            "kimi.plugin_ids".to_string(),
            serde_json::to_value(plugin_ids).unwrap_or(serde_json::Value::Null),
        );
    }
    if let Some(plugins) = &request.plugins {
        vendor.insert(
            "kimi.plugins".to_string(),
            serde_json::to_value(plugins).unwrap_or(serde_json::Value::Null),
        );
    }

    let config = RuntimeConfig {
        model: Some(request.model.clone()),
        vendor,
        ..Default::default()
    };

    WorkOrderBuilder::new(task)
        .model(request.model.clone())
        .config(config)
        .build()
}

// ── Receipt → KimiChatResponse ──────────────────────────────────────────

/// Convert an ABP [`Receipt`] into a [`KimiChatResponse`].
///
/// Walks the receipt trace to reconstruct the assistant message content
/// and any tool calls. Search results are extracted from `ext` metadata
/// on events (keyed as `kimi_search_results`).
pub fn receipt_to_kimi(receipt: &Receipt, model: &str) -> KimiChatResponse {
    let mut content: Option<String> = None;
    let mut tool_calls: Vec<KimiToolCall> = Vec::new();
    let mut finish_reason = "stop".to_string();
    let mut search_results: Vec<KimiSearchResult> = Vec::new();

    for event in &receipt.trace {
        // Extract search results from event ext metadata
        if let Some(ext) = &event.ext {
            if let Some(sr_val) = ext.get("kimi_search_results") {
                if let Ok(srs) = serde_json::from_value::<Vec<KimiSearchResult>>(sr_val.clone()) {
                    search_results.extend(srs);
                }
            }
        }

        match &event.kind {
            AgentEventKind::AssistantMessage { text } => {
                content = Some(text.clone());
            }
            AgentEventKind::AssistantDelta { text } => {
                let c = content.get_or_insert_with(String::new);
                c.push_str(text);
            }
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                input,
                ..
            } => {
                tool_calls.push(KimiToolCall {
                    id: tool_use_id
                        .clone()
                        .unwrap_or_else(|| format!("call_{}", uuid::Uuid::new_v4())),
                    call_type: "function".into(),
                    function: KimiFunctionCall {
                        name: tool_name.clone(),
                        arguments: serde_json::to_string(input).unwrap_or_default(),
                    },
                });
                finish_reason = "tool_calls".to_string();
            }
            AgentEventKind::Error { message, .. } => {
                content = Some(format!("Error: {message}"));
                finish_reason = "stop".to_string();
            }
            _ => {}
        }
    }

    let message = KimiChatChoiceMessage {
        role: "assistant".into(),
        content,
        tool_calls: if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        },
    };

    let usage = usage_from_receipt(&receipt.usage);

    KimiChatResponse {
        id: format!("cmpl-{}", receipt.meta.run_id),
        object: "chat.completion".into(),
        created: receipt.meta.started_at.timestamp() as u64,
        model: model.to_string(),
        choices: vec![KimiChatChoice {
            index: 0,
            message,
            finish_reason: Some(finish_reason),
        }],
        usage: Some(usage),
        search_results: if search_results.is_empty() {
            None
        } else {
            Some(search_results)
        },
    }
}

/// Convert normalized usage to shim [`Usage`].
fn usage_from_receipt(usage: &UsageNormalized) -> Usage {
    let prompt = usage.input_tokens.unwrap_or(0);
    let completion = usage.output_tokens.unwrap_or(0);
    Usage {
        prompt_tokens: prompt,
        completion_tokens: completion,
        total_tokens: prompt + completion,
    }
}

// ── AgentEvent → KimiStreamEvent ────────────────────────────────────────

/// Convert an [`AgentEvent`] into an optional [`KimiStreamEvent`].
///
/// Returns `Some` for assistant deltas, assistant messages, and error events.
/// Returns `None` for event types that have no streaming representation
/// (e.g. `ToolResult`, `FileChanged`, `CommandExecuted`).
///
/// Search results from event `ext` metadata are forwarded into the stream
/// event's `search_results` field.
pub fn agent_event_to_kimi_stream(
    event: &AgentEvent,
    model: &str,
    run_id: &str,
) -> Option<KimiStreamEvent> {
    let created = event.ts.timestamp() as u64;

    // Extract search results from ext if present
    let search_results = event.ext.as_ref().and_then(|ext| {
        ext.get("kimi_search_results").and_then(|v| {
            serde_json::from_value::<Vec<KimiSearchResult>>(v.clone())
                .ok()
                .filter(|srs| !srs.is_empty())
        })
    });

    match &event.kind {
        AgentEventKind::AssistantDelta { text } => Some(KimiStreamEvent {
            id: run_id.to_string(),
            object: "chat.completion.chunk".into(),
            created,
            model: model.to_string(),
            choices: vec![KimiStreamChoice {
                index: 0,
                delta: KimiStreamDelta {
                    role: None,
                    content: Some(text.clone()),
                    tool_calls: None,
                },
                finish_reason: None,
            }],
            usage: None,
            search_results,
        }),
        AgentEventKind::AssistantMessage { text } => Some(KimiStreamEvent {
            id: run_id.to_string(),
            object: "chat.completion.chunk".into(),
            created,
            model: model.to_string(),
            choices: vec![KimiStreamChoice {
                index: 0,
                delta: KimiStreamDelta {
                    role: Some("assistant".into()),
                    content: Some(text.clone()),
                    tool_calls: None,
                },
                finish_reason: None,
            }],
            usage: None,
            search_results,
        }),
        AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            input,
            ..
        } => Some(KimiStreamEvent {
            id: run_id.to_string(),
            object: "chat.completion.chunk".into(),
            created,
            model: model.to_string(),
            choices: vec![KimiStreamChoice {
                index: 0,
                delta: KimiStreamDelta {
                    role: None,
                    content: None,
                    tool_calls: Some(vec![KimiToolCall {
                        id: tool_use_id
                            .clone()
                            .unwrap_or_else(|| format!("call_{}", uuid::Uuid::new_v4())),
                        call_type: "function".into(),
                        function: KimiFunctionCall {
                            name: tool_name.clone(),
                            arguments: serde_json::to_string(input).unwrap_or_default(),
                        },
                    }]),
                },
                finish_reason: None,
            }],
            usage: None,
            search_results: None,
        }),
        AgentEventKind::Error { message, .. } => Some(KimiStreamEvent {
            id: run_id.to_string(),
            object: "chat.completion.chunk".into(),
            created,
            model: model.to_string(),
            choices: vec![KimiStreamChoice {
                index: 0,
                delta: KimiStreamDelta {
                    role: None,
                    content: Some(format!("Error: {message}")),
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: None,
            search_results: None,
        }),
        _ => None,
    }
}

/// Create a final stop [`KimiStreamEvent`] to signal end of stream.
pub fn kimi_stream_stop_event(model: &str, run_id: &str) -> KimiStreamEvent {
    let created = Utc::now().timestamp() as u64;
    KimiStreamEvent {
        id: run_id.to_string(),
        object: "chat.completion.chunk".into(),
        created,
        model: model.to_string(),
        choices: vec![KimiStreamChoice {
            index: 0,
            delta: KimiStreamDelta::default(),
            finish_reason: Some("stop".into()),
        }],
        usage: None,
        search_results: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock_receipt_with_usage;
    use crate::types::KimiPluginConfig;
    use serde_json::json;

    fn make_receipt(events: Vec<AgentEvent>) -> Receipt {
        crate::mock_receipt(events)
    }

    // ── kimi_to_work_order tests ────────────────────────────────────────

    #[test]
    fn basic_request_to_work_order() {
        let req = KimiChatRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![Message::user("Hello Kimi")],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            use_search: None,
            ref_file_ids: None,
            plugin_ids: None,
            plugins: None,
        };
        let wo = kimi_to_work_order(&req);
        assert_eq!(wo.task, "Hello Kimi");
        assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-8k"));
    }

    #[test]
    fn request_with_temperature_and_max_tokens() {
        let req = KimiChatRequest {
            model: "moonshot-v1-128k".into(),
            messages: vec![Message::user("test")],
            temperature: Some(0.7),
            top_p: Some(0.9),
            max_tokens: Some(2048),
            stream: None,
            use_search: None,
            ref_file_ids: None,
            plugin_ids: None,
            plugins: None,
        };
        let wo = kimi_to_work_order(&req);
        assert_eq!(
            wo.config.vendor.get("temperature"),
            Some(&serde_json::Value::from(0.7))
        );
        assert_eq!(
            wo.config.vendor.get("max_tokens"),
            Some(&serde_json::Value::from(2048))
        );
        assert_eq!(
            wo.config.vendor.get("top_p"),
            Some(&serde_json::Value::from(0.9))
        );
    }

    #[test]
    fn request_with_use_search() {
        let req = KimiChatRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![Message::user("What is Rust?")],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            use_search: Some(true),
            ref_file_ids: None,
            plugin_ids: None,
            plugins: None,
        };
        let wo = kimi_to_work_order(&req);
        assert_eq!(
            wo.config.vendor.get("kimi.use_search"),
            Some(&serde_json::Value::from(true))
        );
    }

    #[test]
    fn request_with_file_references() {
        let req = KimiChatRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![Message::user("Summarize this file")],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            use_search: None,
            ref_file_ids: Some(vec!["file-abc123".into(), "file-xyz456".into()]),
            plugin_ids: None,
            plugins: None,
        };
        let wo = kimi_to_work_order(&req);
        let file_ids = wo.config.vendor.get("kimi.ref_file_ids").unwrap();
        let ids: Vec<String> = serde_json::from_value(file_ids.clone()).unwrap();
        assert_eq!(ids, vec!["file-abc123", "file-xyz456"]);
    }

    #[test]
    fn request_with_plugin_ids() {
        let req = KimiChatRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![Message::user("use plugins")],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            use_search: None,
            ref_file_ids: None,
            plugin_ids: Some(vec!["plugin-web".into()]),
            plugins: None,
        };
        let wo = kimi_to_work_order(&req);
        let pids = wo.config.vendor.get("kimi.plugin_ids").unwrap();
        let ids: Vec<String> = serde_json::from_value(pids.clone()).unwrap();
        assert_eq!(ids, vec!["plugin-web"]);
    }

    #[test]
    fn request_with_plugin_configs() {
        let req = KimiChatRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![Message::user("use plugins")],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            use_search: None,
            ref_file_ids: None,
            plugin_ids: None,
            plugins: Some(vec![KimiPluginConfig {
                plugin_id: "plugin-calc".into(),
                name: Some("Calculator".into()),
                enabled: Some(true),
                settings: BTreeMap::new(),
            }]),
        };
        let wo = kimi_to_work_order(&req);
        let plugins_val = wo.config.vendor.get("kimi.plugins").unwrap();
        let plugins: Vec<KimiPluginConfig> = serde_json::from_value(plugins_val.clone()).unwrap();
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].plugin_id, "plugin-calc");
    }

    #[test]
    fn request_defaults_task_when_no_user_message() {
        let req = KimiChatRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![Message::system("You are helpful.")],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            use_search: None,
            ref_file_ids: None,
            plugin_ids: None,
            plugins: None,
        };
        let wo = kimi_to_work_order(&req);
        assert_eq!(wo.task, "kimi completion");
    }

    // ── receipt_to_kimi tests ───────────────────────────────────────────

    #[test]
    fn receipt_to_kimi_simple_message() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Hello!".into(),
            },
            ext: None,
        }];
        let receipt = make_receipt(events);
        let resp = receipt_to_kimi(&receipt, "moonshot-v1-8k");

        assert_eq!(resp.model, "moonshot-v1-8k");
        assert_eq!(resp.object, "chat.completion");
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hello!"));
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
        assert!(resp.search_results.is_none());
    }

    #[test]
    fn receipt_to_kimi_with_tool_calls() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "web_search".into(),
                tool_use_id: Some("call_123".into()),
                parent_tool_use_id: None,
                input: json!({"query": "rust language"}),
            },
            ext: None,
        }];
        let receipt = make_receipt(events);
        let resp = receipt_to_kimi(&receipt, "moonshot-v1-8k");

        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
        let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tcs.len(), 1);
        assert_eq!(tcs[0].id, "call_123");
        assert_eq!(tcs[0].function.name, "web_search");
    }

    #[test]
    fn receipt_to_kimi_with_search_results() {
        let mut ext = BTreeMap::new();
        ext.insert(
            "kimi_search_results".into(),
            json!([
                {"index": 1, "url": "https://example.com", "title": "Example", "snippet": "An example page."}
            ]),
        );

        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Based on search results...".into(),
            },
            ext: Some(ext),
        }];
        let receipt = make_receipt(events);
        let resp = receipt_to_kimi(&receipt, "moonshot-v1-8k");

        let srs = resp.search_results.unwrap();
        assert_eq!(srs.len(), 1);
        assert_eq!(srs[0].url, "https://example.com");
        assert_eq!(srs[0].title.as_deref(), Some("Example"));
        assert_eq!(srs[0].snippet.as_deref(), Some("An example page."));
    }

    #[test]
    fn receipt_to_kimi_with_usage() {
        let usage = UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            cache_read_tokens: None,
            cache_write_tokens: None,
            request_units: None,
            estimated_cost_usd: None,
        };
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "ok".into() },
            ext: None,
        }];
        let receipt = mock_receipt_with_usage(events, usage);
        let resp = receipt_to_kimi(&receipt, "moonshot-v1-8k");

        let u = resp.usage.unwrap();
        assert_eq!(u.prompt_tokens, 100);
        assert_eq!(u.completion_tokens, 50);
        assert_eq!(u.total_tokens, 150);
    }

    #[test]
    fn receipt_to_kimi_error_event() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error {
                message: "rate limit".into(),
                error_code: None,
            },
            ext: None,
        }];
        let receipt = make_receipt(events);
        let resp = receipt_to_kimi(&receipt, "moonshot-v1-8k");

        let content = resp.choices[0].message.content.as_deref().unwrap();
        assert!(content.contains("rate limit"));
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn receipt_to_kimi_delta_concatenation() {
        let events = vec![
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantDelta { text: "Hel".into() },
                ext: None,
            },
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantDelta { text: "lo!".into() },
                ext: None,
            },
        ];
        let receipt = make_receipt(events);
        let resp = receipt_to_kimi(&receipt, "moonshot-v1-8k");

        assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hello!"));
    }

    // ── agent_event_to_kimi_stream tests ────────────────────────────────

    #[test]
    fn stream_delta_event() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "chunk".into(),
            },
            ext: None,
        };
        let se = agent_event_to_kimi_stream(&event, "moonshot-v1-8k", "run-1").unwrap();
        assert_eq!(se.id, "run-1");
        assert_eq!(se.object, "chat.completion.chunk");
        assert_eq!(se.model, "moonshot-v1-8k");
        assert_eq!(se.choices[0].delta.content.as_deref(), Some("chunk"));
        assert!(se.choices[0].delta.role.is_none());
        assert!(se.choices[0].finish_reason.is_none());
    }

    #[test]
    fn stream_full_message_event() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "full".into(),
            },
            ext: None,
        };
        let se = agent_event_to_kimi_stream(&event, "moonshot-v1-8k", "run-1").unwrap();
        assert_eq!(se.choices[0].delta.role.as_deref(), Some("assistant"));
        assert_eq!(se.choices[0].delta.content.as_deref(), Some("full"));
    }

    #[test]
    fn stream_tool_call_event() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "search".into(),
                tool_use_id: Some("call_1".into()),
                parent_tool_use_id: None,
                input: json!({"q": "test"}),
            },
            ext: None,
        };
        let se = agent_event_to_kimi_stream(&event, "moonshot-v1-8k", "run-1").unwrap();
        let tcs = se.choices[0].delta.tool_calls.as_ref().unwrap();
        assert_eq!(tcs.len(), 1);
        assert_eq!(tcs[0].id, "call_1");
        assert_eq!(tcs[0].function.name, "search");
    }

    #[test]
    fn stream_error_event() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error {
                message: "something broke".into(),
                error_code: None,
            },
            ext: None,
        };
        let se = agent_event_to_kimi_stream(&event, "moonshot-v1-8k", "run-1").unwrap();
        let content = se.choices[0].delta.content.as_deref().unwrap();
        assert!(content.contains("something broke"));
        assert_eq!(se.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn stream_ignores_unrepresentable_events() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::FileChanged {
                path: "src/main.rs".into(),
                summary: "edited".into(),
            },
            ext: None,
        };
        assert!(agent_event_to_kimi_stream(&event, "moonshot-v1-8k", "run-1").is_none());
    }

    #[test]
    fn stream_event_with_search_results() {
        let mut ext = BTreeMap::new();
        ext.insert(
            "kimi_search_results".into(),
            json!([{"index": 1, "url": "https://rust-lang.org", "title": "Rust"}]),
        );
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "Rust is...".into(),
            },
            ext: Some(ext),
        };
        let se = agent_event_to_kimi_stream(&event, "moonshot-v1-8k", "run-1").unwrap();
        let srs = se.search_results.unwrap();
        assert_eq!(srs.len(), 1);
        assert_eq!(srs[0].url, "https://rust-lang.org");
    }

    #[test]
    fn stop_event_has_correct_shape() {
        let stop = kimi_stream_stop_event("moonshot-v1-8k", "run-1");
        assert_eq!(stop.id, "run-1");
        assert_eq!(stop.choices[0].finish_reason.as_deref(), Some("stop"));
        assert!(stop.choices[0].delta.content.is_none());
        assert!(stop.choices[0].delta.role.is_none());
    }

    // ── Roundtrip tests ─────────────────────────────────────────────────

    #[test]
    fn roundtrip_request_to_work_order_preserves_model() {
        let req = KimiChatRequest {
            model: "moonshot-v1-128k".into(),
            messages: vec![Message::user("test")],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            use_search: None,
            ref_file_ids: None,
            plugin_ids: None,
            plugins: None,
        };
        let wo = kimi_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-128k"));
    }

    #[test]
    fn roundtrip_multi_turn_uses_last_user_message() {
        let req = KimiChatRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![
                Message::user("First question"),
                Message::assistant("First answer"),
                Message::user("Second question"),
            ],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            use_search: None,
            ref_file_ids: None,
            plugin_ids: None,
            plugins: None,
        };
        let wo = kimi_to_work_order(&req);
        assert_eq!(wo.task, "Second question");
    }

    // ── Serde roundtrip tests ───────────────────────────────────────────

    #[test]
    fn kimi_chat_request_serde_roundtrip() {
        let req = KimiChatRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![Message::user("hi")],
            temperature: Some(0.5),
            top_p: None,
            max_tokens: Some(1024),
            stream: Some(false),
            use_search: Some(true),
            ref_file_ids: Some(vec!["file-1".into()]),
            plugin_ids: Some(vec!["p1".into()]),
            plugins: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: KimiChatRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, req);
    }

    #[test]
    fn kimi_chat_response_serde_roundtrip() {
        let resp = KimiChatResponse {
            id: "cmpl-123".into(),
            object: "chat.completion".into(),
            created: 1700000000,
            model: "moonshot-v1-8k".into(),
            choices: vec![KimiChatChoice {
                index: 0,
                message: KimiChatChoiceMessage {
                    role: "assistant".into(),
                    content: Some("hello".into()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: Some(Usage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            }),
            search_results: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: KimiChatResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, resp);
    }

    #[test]
    fn kimi_stream_event_serde_roundtrip() {
        let event = KimiStreamEvent {
            id: "chunk-1".into(),
            object: "chat.completion.chunk".into(),
            created: 1700000000,
            model: "moonshot-v1-8k".into(),
            choices: vec![KimiStreamChoice {
                index: 0,
                delta: KimiStreamDelta {
                    role: Some("assistant".into()),
                    content: Some("hi".into()),
                    tool_calls: None,
                },
                finish_reason: None,
            }],
            usage: None,
            search_results: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        let parsed: KimiStreamEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, event);
    }

    #[test]
    fn search_result_serde_roundtrip() {
        let sr = KimiSearchResult {
            index: 1,
            url: "https://example.com".into(),
            title: Some("Example".into()),
            snippet: Some("A snippet.".into()),
        };
        let json = serde_json::to_string(&sr).unwrap();
        let parsed: KimiSearchResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, sr);
    }

    #[test]
    fn search_result_optional_fields_omitted() {
        let sr = KimiSearchResult {
            index: 1,
            url: "https://example.com".into(),
            title: None,
            snippet: None,
        };
        let json = serde_json::to_string(&sr).unwrap();
        assert!(!json.contains("title"));
        assert!(!json.contains("snippet"));
    }

    #[test]
    fn file_reference_serde_roundtrip() {
        use crate::types::KimiFileReference;
        let fr = KimiFileReference {
            file_id: "file-abc".into(),
            filename: Some("report.pdf".into()),
            purpose: Some("file-extract".into()),
        };
        let json = serde_json::to_string(&fr).unwrap();
        let parsed: KimiFileReference = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, fr);
    }

    #[test]
    fn plugin_config_serde_roundtrip() {
        let mut settings = BTreeMap::new();
        settings.insert("verbose".into(), json!(true));
        let pc = KimiPluginConfig {
            plugin_id: "plugin-calc".into(),
            name: Some("Calculator".into()),
            enabled: Some(true),
            settings,
        };
        let json = serde_json::to_string(&pc).unwrap();
        let parsed: KimiPluginConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, pc);
    }

    #[test]
    fn plugin_config_empty_settings_omitted() {
        let pc = KimiPluginConfig {
            plugin_id: "plugin-x".into(),
            name: None,
            enabled: None,
            settings: BTreeMap::new(),
        };
        let json = serde_json::to_string(&pc).unwrap();
        assert!(!json.contains("settings"));
    }
}
