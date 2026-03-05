// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code, unused_imports)]
//! Translation layer between Codex-specific shim types and ABP core types.
//!
//! Provides named conversion functions following the shim convention:
//!
//! - `codex_to_work_order` — Extended Codex request → ABP `WorkOrder`
//! - `receipt_to_codex` — ABP `Receipt` → Extended Codex response
//! - `agent_event_to_codex_stream` — Single `AgentEvent` → Codex stream event

use std::collections::BTreeMap;

use abp_codex_sdk::dialect::{
    CodexContentPart, CodexRequest, CodexResponse, CodexResponseItem, CodexStreamDelta,
    CodexStreamEvent, CodexUsage,
};
use abp_core::ir::IrRole;
use abp_core::{
    AgentEvent, AgentEventKind, ContextSnippet, Receipt, RuntimeConfig, UsageNormalized, WorkOrder,
    WorkOrderBuilder,
};
use chrono::Utc;

use crate::convert::{events_to_stream_events, receipt_to_response, request_to_ir};
use crate::types::{
    CodexContextItem, CodexExtendedRequest, CodexExtendedResponse, CodexSandboxResult,
    CodexShimStreamEvent, CodexToolCall, CodexToolResult, Usage,
};

// ── Extended request → WorkOrder ────────────────────────────────────────

/// Convert a [`CodexExtendedRequest`] into an ABP [`WorkOrder`].
///
/// Maps all Codex-specific fields:
/// - `input` → task text (from last user message)
/// - `instructions` → system-level context snippet
/// - `context` → context packet file entries and snippets
/// - `model`, `temperature`, `max_output_tokens` → runtime config
/// - `sandbox` → vendor config sandbox settings
pub fn codex_to_work_order(request: &CodexExtendedRequest) -> WorkOrder {
    let base_request = request.to_base_request();
    let conv = request_to_ir(&base_request);

    let task = conv
        .messages
        .iter()
        .rev()
        .find(|m| m.role == IrRole::User)
        .map(|m| m.text_content())
        .unwrap_or_else(|| "codex completion".into());

    let mut builder = WorkOrderBuilder::new(task).model(request.model.clone());

    // Build vendor config
    let mut vendor = BTreeMap::new();
    if let Some(temp) = request.temperature {
        vendor.insert("temperature".to_string(), serde_json::Value::from(temp));
    }
    if let Some(max) = request.max_output_tokens {
        vendor.insert(
            "max_output_tokens".to_string(),
            serde_json::Value::from(max),
        );
    }
    if let Some(sandbox) = &request.sandbox {
        if let Ok(v) = serde_json::to_value(sandbox) {
            vendor.insert("sandbox".to_string(), v);
        }
    }
    for (k, v) in &request.metadata {
        vendor.insert(k.clone(), v.clone());
    }

    let config = RuntimeConfig {
        model: Some(request.model.clone()),
        vendor,
        ..Default::default()
    };
    builder = builder.config(config);

    // Build context packet from instructions + context items
    let mut snippets = Vec::new();
    if let Some(instructions) = &request.instructions {
        snippets.push(ContextSnippet {
            name: "instructions".into(),
            content: instructions.clone(),
        });
    }
    for ctx in &request.context {
        if let Some(content) = &ctx.content {
            snippets.push(ContextSnippet {
                name: ctx.path.clone(),
                content: content.clone(),
            });
        }
    }
    let files: Vec<String> = request.context.iter().map(|c| c.path.clone()).collect();
    let context = abp_core::ContextPacket { files, snippets };
    builder = builder.context(context);

    builder.build()
}

// ── Receipt → Extended Codex response ───────────────────────────────────

/// Convert an ABP [`Receipt`] into a [`CodexExtendedResponse`].
///
/// Maps the receipt trace into Codex output items and extracts usage,
/// sandbox results (from artifacts), and outcome status.
pub fn receipt_to_codex(receipt: &Receipt, model: &str) -> CodexExtendedResponse {
    let base = receipt_to_response(receipt, model);

    let usage = base.usage.map(|u| Usage {
        input_tokens: u.input_tokens,
        output_tokens: u.output_tokens,
        total_tokens: u.total_tokens,
    });

    // Extract sandbox result from receipt metadata if present
    let sandbox_result = if receipt.verification.harness_ok || !receipt.artifacts.is_empty() {
        let files_modified: Vec<String> = receipt
            .artifacts
            .iter()
            .filter(|a| a.kind == "patch" || a.kind == "file")
            .map(|a| a.path.clone())
            .collect();
        if files_modified.is_empty() {
            None
        } else {
            Some(CodexSandboxResult {
                exit_code: Some(0),
                duration_ms: Some(receipt.meta.duration_ms),
                files_modified,
            })
        }
    } else {
        None
    };

    let status = match receipt.outcome {
        abp_core::Outcome::Complete => Some("completed".into()),
        abp_core::Outcome::Partial => Some("incomplete".into()),
        abp_core::Outcome::Failed => Some("failed".into()),
    };

    CodexExtendedResponse {
        id: base.id,
        model: base.model,
        output: base.output,
        usage,
        status,
        sandbox_result,
        metadata: BTreeMap::new(),
    }
}

// ── AgentEvent → Codex stream event ─────────────────────────────────────

/// Convert a single [`AgentEvent`] into an optional [`CodexShimStreamEvent`].
///
/// Returns `None` for event kinds with no Codex streaming equivalent
/// (e.g. `FileChanged`, `CommandExecuted`, `Warning`).
///
/// The `sequence` parameter provides the monotonic ordering number for the
/// stream event, and `model` is echoed into the event metadata.
pub fn agent_event_to_codex_stream(
    event: &AgentEvent,
    model: &str,
    sequence: u64,
) -> Option<CodexShimStreamEvent> {
    match &event.kind {
        AgentEventKind::RunStarted { .. } => Some(CodexShimStreamEvent::ResponseCreated {
            sequence,
            response_id: format!("resp_{}", uuid::Uuid::new_v4()),
            model: model.to_string(),
        }),
        AgentEventKind::AssistantDelta { text } => Some(CodexShimStreamEvent::TextDelta {
            sequence,
            output_index: 0,
            text: text.clone(),
        }),
        AgentEventKind::AssistantMessage { text } => Some(CodexShimStreamEvent::OutputItemDone {
            sequence,
            output_index: 0,
            item: CodexResponseItem::Message {
                role: "assistant".into(),
                content: vec![CodexContentPart::OutputText { text: text.clone() }],
            },
        }),
        AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            input,
            ..
        } => Some(CodexShimStreamEvent::OutputItemDone {
            sequence,
            output_index: 0,
            item: CodexResponseItem::FunctionCall {
                id: tool_use_id
                    .clone()
                    .unwrap_or_else(|| format!("fc_{}", uuid::Uuid::new_v4())),
                call_id: None,
                name: tool_name.clone(),
                arguments: serde_json::to_string(input).unwrap_or_default(),
            },
        }),
        AgentEventKind::RunCompleted { .. } => Some(CodexShimStreamEvent::ResponseCompleted {
            sequence,
            response_id: String::new(),
            usage: None,
        }),
        AgentEventKind::Error { message, .. } => Some(CodexShimStreamEvent::Error {
            sequence,
            message: message.clone(),
            code: None,
        }),
        _ => None,
    }
}

// ── Tool call/result helpers ────────────────────────────────────────────

/// Convert a [`CodexToolCall`] into an ABP [`AgentEvent`] (tool call kind).
pub fn tool_call_to_event(tc: &CodexToolCall) -> AgentEvent {
    let input = serde_json::from_str(&tc.arguments)
        .unwrap_or(serde_json::Value::String(tc.arguments.clone()));
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: tc.name.clone(),
            tool_use_id: Some(tc.id.clone()),
            parent_tool_use_id: None,
            input,
        },
        ext: None,
    }
}

/// Convert a [`CodexToolResult`] into an ABP [`AgentEvent`] (tool result kind).
pub fn tool_result_to_event(tr: &CodexToolResult) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolResult {
            tool_name: "function".into(),
            tool_use_id: Some(tr.call_id.clone()),
            output: serde_json::Value::String(tr.output.clone()),
            is_error: tr.is_error,
        },
        ext: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CodexContextItem, CodexSandboxConfig};
    use abp_core::{Outcome, UsageNormalized};

    fn make_extended_request() -> CodexExtendedRequest {
        CodexExtendedRequest {
            model: "codex-mini-latest".into(),
            input: vec![abp_codex_sdk::dialect::CodexInputItem::Message {
                role: "user".into(),
                content: "Write tests".into(),
            }],
            instructions: Some("Be concise.".into()),
            context: vec![CodexContextItem {
                path: "src/lib.rs".into(),
                content: Some("fn main() {}".into()),
            }],
            max_output_tokens: Some(2048),
            temperature: Some(0.5),
            tools: vec![],
            text: None,
            sandbox: Some(CodexSandboxConfig::default()),
            metadata: BTreeMap::new(),
        }
    }

    fn make_mock_receipt(events: Vec<AgentEvent>) -> Receipt {
        crate::mock_receipt(events)
    }

    // ── codex_to_work_order tests ───────────────────────────────────────

    #[test]
    fn codex_to_work_order_extracts_task() {
        let req = make_extended_request();
        let wo = codex_to_work_order(&req);
        assert_eq!(wo.task, "Write tests");
    }

    #[test]
    fn codex_to_work_order_maps_model() {
        let req = make_extended_request();
        let wo = codex_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("codex-mini-latest"));
    }

    #[test]
    fn codex_to_work_order_maps_temperature() {
        let req = make_extended_request();
        let wo = codex_to_work_order(&req);
        assert_eq!(
            wo.config.vendor.get("temperature"),
            Some(&serde_json::Value::from(0.5))
        );
    }

    #[test]
    fn codex_to_work_order_maps_max_output_tokens() {
        let req = make_extended_request();
        let wo = codex_to_work_order(&req);
        assert_eq!(
            wo.config.vendor.get("max_output_tokens"),
            Some(&serde_json::Value::from(2048))
        );
    }

    #[test]
    fn codex_to_work_order_includes_instructions_in_context() {
        let req = make_extended_request();
        let wo = codex_to_work_order(&req);
        let instr = wo
            .context
            .snippets
            .iter()
            .find(|s| s.name == "instructions");
        assert!(instr.is_some());
        assert_eq!(instr.unwrap().content, "Be concise.");
    }

    #[test]
    fn codex_to_work_order_includes_file_context() {
        let req = make_extended_request();
        let wo = codex_to_work_order(&req);
        assert!(wo.context.files.contains(&"src/lib.rs".to_string()));
        let snippet = wo.context.snippets.iter().find(|s| s.name == "src/lib.rs");
        assert!(snippet.is_some());
        assert_eq!(snippet.unwrap().content, "fn main() {}");
    }

    #[test]
    fn codex_to_work_order_includes_sandbox_in_vendor() {
        let req = make_extended_request();
        let wo = codex_to_work_order(&req);
        assert!(wo.config.vendor.contains_key("sandbox"));
    }

    #[test]
    fn codex_to_work_order_without_optional_fields() {
        let req = CodexExtendedRequest {
            model: "o3-mini".into(),
            input: vec![abp_codex_sdk::dialect::CodexInputItem::Message {
                role: "user".into(),
                content: "Hello".into(),
            }],
            instructions: None,
            context: vec![],
            max_output_tokens: None,
            temperature: None,
            tools: vec![],
            text: None,
            sandbox: None,
            metadata: BTreeMap::new(),
        };
        let wo = codex_to_work_order(&req);
        assert_eq!(wo.task, "Hello");
        assert_eq!(wo.config.model.as_deref(), Some("o3-mini"));
        assert!(wo.context.snippets.is_empty());
        assert!(wo.context.files.is_empty());
    }

    // ── receipt_to_codex tests ──────────────────────────────────────────

    #[test]
    fn receipt_to_codex_maps_assistant_message() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Done!".into(),
            },
            ext: None,
        }];
        let receipt = make_mock_receipt(events);
        let resp = receipt_to_codex(&receipt, "codex-mini-latest");

        assert_eq!(resp.model, "codex-mini-latest");
        assert_eq!(resp.status.as_deref(), Some("completed"));
        assert_eq!(resp.output.len(), 1);
        match &resp.output[0] {
            CodexResponseItem::Message { content, .. } => match &content[0] {
                CodexContentPart::OutputText { text } => assert_eq!(text, "Done!"),
            },
            other => panic!("expected Message, got {other:?}"),
        }
    }

    #[test]
    fn receipt_to_codex_maps_tool_call() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "shell".into(),
                tool_use_id: Some("fc_1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"command": "ls"}),
            },
            ext: None,
        }];
        let receipt = make_mock_receipt(events);
        let resp = receipt_to_codex(&receipt, "codex-mini-latest");

        assert_eq!(resp.output.len(), 1);
        match &resp.output[0] {
            CodexResponseItem::FunctionCall {
                id,
                name,
                arguments,
                ..
            } => {
                assert_eq!(id, "fc_1");
                assert_eq!(name, "shell");
                assert!(arguments.contains("ls"));
            }
            other => panic!("expected FunctionCall, got {other:?}"),
        }
    }

    #[test]
    fn receipt_to_codex_maps_usage() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "ok".into() },
            ext: None,
        }];
        let receipt = crate::mock_receipt_with_usage(
            events,
            UsageNormalized {
                input_tokens: Some(100),
                output_tokens: Some(50),
                cache_read_tokens: None,
                cache_write_tokens: None,
                request_units: None,
                estimated_cost_usd: None,
            },
        );
        let resp = receipt_to_codex(&receipt, "codex-mini-latest");
        let u = resp.usage.unwrap();
        assert_eq!(u.input_tokens, 100);
        assert_eq!(u.output_tokens, 50);
        assert_eq!(u.total_tokens, 150);
    }

    #[test]
    fn receipt_to_codex_maps_error() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error {
                message: "rate limit".into(),
                error_code: None,
            },
            ext: None,
        }];
        let receipt = make_mock_receipt(events);
        let resp = receipt_to_codex(&receipt, "codex-mini-latest");
        match &resp.output[0] {
            CodexResponseItem::Message { content, .. } => match &content[0] {
                CodexContentPart::OutputText { text } => {
                    assert!(text.contains("rate limit"));
                }
            },
            other => panic!("expected Message, got {other:?}"),
        }
    }

    // ── agent_event_to_codex_stream tests ───────────────────────────────

    #[test]
    fn stream_run_started() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "started".into(),
            },
            ext: None,
        };
        let se = agent_event_to_codex_stream(&event, "codex-mini-latest", 0);
        assert!(matches!(
            se,
            Some(CodexShimStreamEvent::ResponseCreated { .. })
        ));
    }

    #[test]
    fn stream_assistant_delta() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "Hello".into(),
            },
            ext: None,
        };
        let se = agent_event_to_codex_stream(&event, "codex-mini-latest", 1).unwrap();
        match se {
            CodexShimStreamEvent::TextDelta { sequence, text, .. } => {
                assert_eq!(sequence, 1);
                assert_eq!(text, "Hello");
            }
            other => panic!("expected TextDelta, got {other:?}"),
        }
    }

    #[test]
    fn stream_assistant_message() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Done".into(),
            },
            ext: None,
        };
        let se = agent_event_to_codex_stream(&event, "codex-mini-latest", 2).unwrap();
        assert!(matches!(se, CodexShimStreamEvent::OutputItemDone { .. }));
    }

    #[test]
    fn stream_tool_call() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read".into(),
                tool_use_id: Some("fc_1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "a.rs"}),
            },
            ext: None,
        };
        let se = agent_event_to_codex_stream(&event, "codex-mini-latest", 3).unwrap();
        match se {
            CodexShimStreamEvent::OutputItemDone { item, .. } => match item {
                CodexResponseItem::FunctionCall { name, .. } => assert_eq!(name, "read"),
                other => panic!("expected FunctionCall, got {other:?}"),
            },
            other => panic!("expected OutputItemDone, got {other:?}"),
        }
    }

    #[test]
    fn stream_run_completed() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        };
        let se = agent_event_to_codex_stream(&event, "codex-mini-latest", 4);
        assert!(matches!(
            se,
            Some(CodexShimStreamEvent::ResponseCompleted { .. })
        ));
    }

    #[test]
    fn stream_error() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error {
                message: "fail".into(),
                error_code: None,
            },
            ext: None,
        };
        let se = agent_event_to_codex_stream(&event, "codex-mini-latest", 5).unwrap();
        match se {
            CodexShimStreamEvent::Error {
                message, sequence, ..
            } => {
                assert_eq!(message, "fail");
                assert_eq!(sequence, 5);
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn stream_unsupported_event_returns_none() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::FileChanged {
                path: "a.rs".into(),
                summary: "changed".into(),
            },
            ext: None,
        };
        assert!(agent_event_to_codex_stream(&event, "codex-mini-latest", 0).is_none());
    }

    // ── Tool call/result conversion tests ───────────────────────────────

    #[test]
    fn tool_call_to_event_roundtrip() {
        let tc = CodexToolCall {
            id: "tc_1".into(),
            name: "shell".into(),
            arguments: r#"{"command":"ls"}"#.into(),
            requires_approval: false,
            sandbox_override: None,
        };
        let event = tool_call_to_event(&tc);
        match &event.kind {
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                input,
                ..
            } => {
                assert_eq!(tool_name, "shell");
                assert_eq!(tool_use_id.as_deref(), Some("tc_1"));
                assert_eq!(input["command"], "ls");
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn tool_result_to_event_roundtrip() {
        let tr = CodexToolResult {
            call_id: "tc_1".into(),
            output: "file1.rs\nfile2.rs".into(),
            is_error: false,
            exit_code: Some(0),
            duration_ms: Some(50),
        };
        let event = tool_result_to_event(&tr);
        match &event.kind {
            AgentEventKind::ToolResult {
                tool_use_id,
                output,
                is_error,
                ..
            } => {
                assert_eq!(tool_use_id.as_deref(), Some("tc_1"));
                assert_eq!(
                    output,
                    &serde_json::Value::String("file1.rs\nfile2.rs".into())
                );
                assert!(!is_error);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn tool_result_error_flag() {
        let tr = CodexToolResult {
            call_id: "tc_2".into(),
            output: "command not found".into(),
            is_error: true,
            exit_code: Some(127),
            duration_ms: None,
        };
        let event = tool_result_to_event(&tr);
        match &event.kind {
            AgentEventKind::ToolResult { is_error, .. } => {
                assert!(is_error);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    // ── Serialization snapshot tests ────────────────────────────────────

    #[test]
    fn extended_request_serializes() {
        let req = make_extended_request();
        let json = serde_json::to_string_pretty(&req).unwrap();
        assert!(json.contains("codex-mini-latest"));
        assert!(json.contains("Be concise."));
        assert!(json.contains("src/lib.rs"));
        assert!(json.contains("timeout_seconds"));
    }

    #[test]
    fn extended_request_roundtrip_serde() {
        let req = make_extended_request();
        let json = serde_json::to_string(&req).unwrap();
        let decoded: CodexExtendedRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.model, req.model);
        assert_eq!(decoded.instructions, req.instructions);
        assert_eq!(decoded.context.len(), req.context.len());
        assert_eq!(decoded.max_output_tokens, req.max_output_tokens);
        assert_eq!(decoded.temperature, req.temperature);
        assert_eq!(decoded.sandbox.is_some(), req.sandbox.is_some());
    }

    #[test]
    fn sandbox_config_serializes_defaults() {
        let cfg = CodexSandboxConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(json.contains("timeout_seconds"));
        assert!(json.contains("300"));
        let decoded: CodexSandboxConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg, decoded);
    }

    #[test]
    fn tool_call_serializes() {
        let tc = CodexToolCall {
            id: "tc_1".into(),
            name: "shell".into(),
            arguments: r#"{"cmd":"ls"}"#.into(),
            requires_approval: true,
            sandbox_override: None,
        };
        let json = serde_json::to_string(&tc).unwrap();
        assert!(json.contains("requires_approval"));
        assert!(json.contains("true"));
        let decoded: CodexToolCall = serde_json::from_str(&json).unwrap();
        assert_eq!(tc, decoded);
    }

    #[test]
    fn tool_result_serializes() {
        let tr = CodexToolResult {
            call_id: "tc_1".into(),
            output: "ok".into(),
            is_error: false,
            exit_code: Some(0),
            duration_ms: Some(100),
        };
        let json = serde_json::to_string(&tr).unwrap();
        let decoded: CodexToolResult = serde_json::from_str(&json).unwrap();
        assert_eq!(tr, decoded);
    }

    #[test]
    fn stream_event_serializes() {
        let event = CodexShimStreamEvent::TextDelta {
            sequence: 1,
            output_index: 0,
            text: "hello".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("text_delta"));
        assert!(json.contains("hello"));
        let decoded: CodexShimStreamEvent = serde_json::from_str(&json).unwrap();
        match decoded {
            CodexShimStreamEvent::TextDelta {
                sequence,
                output_index,
                text,
            } => {
                assert_eq!(sequence, 1);
                assert_eq!(output_index, 0);
                assert_eq!(text, "hello");
            }
            other => panic!("expected TextDelta, got {other:?}"),
        }
    }

    #[test]
    fn extended_response_serializes() {
        let resp = CodexExtendedResponse {
            id: "resp_1".into(),
            model: "codex-mini-latest".into(),
            output: vec![CodexResponseItem::Message {
                role: "assistant".into(),
                content: vec![CodexContentPart::OutputText { text: "hi".into() }],
            }],
            usage: Some(Usage {
                input_tokens: 10,
                output_tokens: 5,
                total_tokens: 15,
            }),
            status: Some("completed".into()),
            sandbox_result: None,
            metadata: BTreeMap::new(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let decoded: CodexExtendedResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.id, "resp_1");
        assert_eq!(decoded.model, "codex-mini-latest");
        assert_eq!(decoded.output.len(), 1);
        assert_eq!(decoded.usage.unwrap().total_tokens, 15);
        assert_eq!(decoded.status.as_deref(), Some("completed"));
    }

    #[test]
    fn optional_fields_skipped_when_none() {
        let req = CodexExtendedRequest {
            model: "codex-mini-latest".into(),
            input: vec![],
            instructions: None,
            context: vec![],
            max_output_tokens: None,
            temperature: None,
            tools: vec![],
            text: None,
            sandbox: None,
            metadata: BTreeMap::new(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.contains("instructions"));
        assert!(!json.contains("sandbox"));
        assert!(!json.contains("temperature"));
        assert!(!json.contains("max_output_tokens"));
    }
}
