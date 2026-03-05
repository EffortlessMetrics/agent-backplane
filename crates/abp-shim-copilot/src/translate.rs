// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code, unused_imports)]
//! Translation layer between Copilot-specific extended types and ABP core types.
//!
//! This module provides:
//!
//! - [`copilot_to_work_order`] — Convert [`CopilotChatRequest`] → ABP [`WorkOrder`]
//! - [`receipt_to_copilot`] — Convert ABP [`Receipt`] → [`CopilotChatResponse`]
//! - [`agent_event_to_copilot_stream`] — Convert [`AgentEvent`] → [`CopilotLocalStreamEvent`]

use std::collections::BTreeMap;

use abp_copilot_sdk::dialect::{
    CopilotError, CopilotFunctionCall, CopilotReference, CopilotReferenceType,
};
use abp_core::{
    AgentEvent, AgentEventKind, ContextPacket, ContextSnippet, Receipt, RuntimeConfig, WorkOrder,
    WorkOrderBuilder,
};

use crate::types::{
    CopilotChatRequest, CopilotChatResponse, CopilotCodeReference, CopilotDocContext,
    CopilotIntent, CopilotLocalStreamEvent, CopilotResponseMetadata, CopilotSkill,
};

// ── CopilotChatRequest → WorkOrder ──────────────────────────────────────

/// Convert a [`CopilotChatRequest`] into an ABP [`WorkOrder`].
///
/// Maps:
/// - `messages` → `work_order.task` (extracted from last user message)
/// - `model` → `work_order.config.model`
/// - `intent` → `work_order.config.vendor["copilot_intent"]`
/// - `doc_context` → `work_order.context` (current file) + `vendor["copilot_doc_context"]`
/// - `references` → `work_order.context.files` / `vendor["copilot_references"]`
/// - `skills` → `work_order.config.vendor["copilot_skills"]`
/// - `temperature`, `max_tokens` → `work_order.config.vendor`
pub fn copilot_to_work_order(req: &CopilotChatRequest) -> WorkOrder {
    let task = extract_task(req);
    let mut builder = WorkOrderBuilder::new(task).model(req.model.clone());

    let mut vendor = BTreeMap::new();

    // Store dialect marker
    vendor.insert(
        "dialect".to_string(),
        serde_json::Value::String("copilot".into()),
    );

    // Map intent
    if let Some(intent) = &req.intent {
        if let Ok(v) = serde_json::to_value(intent) {
            vendor.insert("copilot_intent".to_string(), v);
        }
    }

    // Map doc context
    if let Some(doc_ctx) = &req.doc_context {
        if let Ok(v) = serde_json::to_value(doc_ctx) {
            vendor.insert("copilot_doc_context".to_string(), v);
        }
    }

    // Map code references
    if !req.references.is_empty() {
        if let Ok(v) = serde_json::to_value(&req.references) {
            vendor.insert("copilot_references".to_string(), v);
        }
    }

    // Map skills
    if !req.skills.is_empty() {
        if let Ok(v) = serde_json::to_value(&req.skills) {
            vendor.insert("copilot_skills".to_string(), v);
        }
    }

    // Map temperature
    if let Some(temp) = req.temperature {
        vendor.insert("temperature".to_string(), serde_json::Value::from(temp));
    }

    // Map max_tokens
    if let Some(max) = req.max_tokens {
        vendor.insert("max_tokens".to_string(), serde_json::Value::from(max));
    }

    // Map stream
    if let Some(stream) = req.stream {
        vendor.insert("stream".to_string(), serde_json::Value::from(stream));
    }

    // Build context from references and doc context
    let mut files = Vec::new();
    let mut snippets = Vec::new();

    for code_ref in &req.references {
        files.push(code_ref.path.clone());
        if let Some(content) = &code_ref.content {
            snippets.push(ContextSnippet {
                name: code_ref.path.clone(),
                content: content.clone(),
            });
        }
    }

    if let Some(doc_ctx) = &req.doc_context {
        if !files.contains(&doc_ctx.uri) {
            files.push(doc_ctx.uri.clone());
        }
        if let Some(content) = &doc_ctx.content {
            snippets.push(ContextSnippet {
                name: doc_ctx.uri.clone(),
                content: content.clone(),
            });
        }
    }

    let context = ContextPacket { files, snippets };

    let config = RuntimeConfig {
        model: Some(req.model.clone()),
        vendor,
        ..Default::default()
    };
    builder = builder.config(config).context(context);

    builder.build()
}

/// Extract the task string from the last user message.
fn extract_task(req: &CopilotChatRequest) -> String {
    // Prefer last user message; fall back to intent description
    let user_msg = req
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content.clone());

    if let Some(msg) = user_msg {
        if !msg.is_empty() {
            return msg;
        }
    }

    if let Some(intent) = &req.intent {
        return format!("copilot {intent}");
    }

    "copilot completion".into()
}

// ── Receipt → CopilotChatResponse ───────────────────────────────────────

/// Convert an ABP [`Receipt`] into a [`CopilotChatResponse`].
///
/// Walks the receipt trace to reconstruct the assistant message, errors,
/// and function calls. Optionally attaches the model and intent metadata.
pub fn receipt_to_copilot(receipt: &Receipt, model: &str) -> CopilotChatResponse {
    let mut message = String::new();
    let mut errors: Vec<CopilotError> = Vec::new();
    let mut function_call: Option<CopilotFunctionCall> = None;
    let mut references: Vec<CopilotReference> = Vec::new();

    for event in &receipt.trace {
        match &event.kind {
            AgentEventKind::AssistantMessage { text } => {
                message = text.clone();
            }
            AgentEventKind::AssistantDelta { text } => {
                message.push_str(text);
            }
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                input,
                ..
            } => {
                function_call = Some(CopilotFunctionCall {
                    name: tool_name.clone(),
                    arguments: serde_json::to_string(input).unwrap_or_default(),
                    id: tool_use_id.clone(),
                });
            }
            AgentEventKind::Error {
                message: msg,
                error_code,
            } => {
                errors.push(CopilotError {
                    error_type: "backend_error".into(),
                    message: msg.clone(),
                    code: error_code.as_ref().map(|c| c.to_string()),
                    identifier: None,
                });
            }
            _ => {
                // Extract copilot_references from ext if present
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

    // Build metadata
    let metadata = CopilotResponseMetadata {
        intent: None,
        model: Some(model.to_string()),
        ext: BTreeMap::new(),
    };

    CopilotChatResponse {
        message,
        copilot_references: references,
        copilot_errors: errors,
        function_call,
        metadata: Some(metadata),
    }
}

// ── AgentEvent → CopilotLocalStreamEvent ────────────────────────────────

/// Convert a single [`AgentEvent`] into zero or more [`CopilotLocalStreamEvent`]s.
///
/// Most events produce exactly one stream event. Events without a Copilot
/// streaming equivalent (e.g. `FileChanged`, `CommandExecuted`) are skipped.
pub fn agent_event_to_copilot_stream(event: &AgentEvent) -> Vec<CopilotLocalStreamEvent> {
    match &event.kind {
        AgentEventKind::AssistantDelta { text } => {
            vec![CopilotLocalStreamEvent::TextDelta { text: text.clone() }]
        }
        AgentEventKind::AssistantMessage { text } => {
            vec![CopilotLocalStreamEvent::TextDelta { text: text.clone() }]
        }
        AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            input,
            ..
        } => {
            vec![CopilotLocalStreamEvent::FunctionCall {
                function_call: CopilotFunctionCall {
                    name: tool_name.clone(),
                    arguments: serde_json::to_string(input).unwrap_or_default(),
                    id: tool_use_id.clone(),
                },
            }]
        }
        AgentEventKind::Error { message, .. } => {
            vec![CopilotLocalStreamEvent::CopilotErrors {
                errors: vec![CopilotError {
                    error_type: "backend_error".into(),
                    message: message.clone(),
                    code: None,
                    identifier: None,
                }],
            }]
        }
        AgentEventKind::RunStarted { .. } => {
            // Extract references from ext if present
            let references = event
                .ext
                .as_ref()
                .and_then(|ext| ext.get("copilot_references"))
                .and_then(|v| serde_json::from_value::<Vec<CopilotReference>>(v.clone()).ok())
                .unwrap_or_default();
            vec![CopilotLocalStreamEvent::CopilotReferences { references }]
        }
        AgentEventKind::RunCompleted { .. } => {
            vec![CopilotLocalStreamEvent::Done {}]
        }
        _ => vec![],
    }
}

/// Build a complete stream of [`CopilotLocalStreamEvent`]s from a receipt trace.
///
/// Prepends a references event and appends a done event, with the trace events
/// in between.
pub fn receipt_trace_to_copilot_stream(
    events: &[AgentEvent],
    model: &str,
) -> Vec<CopilotLocalStreamEvent> {
    let mut stream = Vec::new();

    // Opening references event
    stream.push(CopilotLocalStreamEvent::CopilotReferences { references: vec![] });

    // Map all trace events
    for event in events {
        stream.extend(agent_event_to_copilot_stream(event));
    }

    // Metadata event
    stream.push(CopilotLocalStreamEvent::Metadata {
        metadata: CopilotResponseMetadata {
            intent: None,
            model: Some(model.to_string()),
            ext: BTreeMap::new(),
        },
    });

    // Final done event
    stream.push(CopilotLocalStreamEvent::Done {});

    stream
}

// ── Helpers ─────────────────────────────────────────────────────────────

/// Convert a [`CopilotCodeReference`] to the SDK's generic [`CopilotReference`].
pub fn code_ref_to_sdk(code_ref: &CopilotCodeReference) -> CopilotReference {
    let mut data = BTreeMap::new();
    data.insert(
        "path".to_string(),
        serde_json::Value::String(code_ref.path.clone()),
    );
    if let Some(lang) = &code_ref.language {
        data.insert(
            "language".to_string(),
            serde_json::Value::String(lang.clone()),
        );
    }
    if let Some(content) = &code_ref.content {
        data.insert(
            "content".to_string(),
            serde_json::Value::String(content.clone()),
        );
    }

    let mut metadata_map = BTreeMap::new();
    if let Some(sel) = &code_ref.selection {
        if let Ok(v) = serde_json::to_value(sel) {
            metadata_map.insert("selection".to_string(), v);
        }
    }

    CopilotReference {
        ref_type: CopilotReferenceType::File,
        id: format!("ref-{}", code_ref.path.replace('/', "-")),
        data: serde_json::to_value(data).unwrap_or(serde_json::Value::Null),
        metadata: if metadata_map.is_empty() {
            None
        } else {
            Some(metadata_map)
        },
    }
}

/// Convert a [`CopilotSkill`] to a vendor config entry.
pub fn skill_to_vendor_value(skill: &CopilotSkill) -> serde_json::Value {
    serde_json::to_value(skill).unwrap_or(serde_json::Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SelectionRange;
    use abp_copilot_sdk::dialect::{CopilotFunctionCall, CopilotMessage};
    use abp_core::{AgentEvent, AgentEventKind, BackendIdentity, UsageNormalized};
    use chrono::Utc;

    fn make_messages(texts: &[(&str, &str)]) -> Vec<CopilotMessage> {
        texts
            .iter()
            .map(|(role, content)| CopilotMessage {
                role: role.to_string(),
                content: content.to_string(),
                name: None,
                copilot_references: vec![],
            })
            .collect()
    }

    fn make_receipt(events: Vec<AgentEvent>) -> Receipt {
        crate::mock_receipt(events)
    }

    // ── 1. Intent enum mapping ──────────────────────────────────────────

    #[test]
    fn intent_explain_serializes() {
        let intent = CopilotIntent::Explain;
        let json = serde_json::to_string(&intent).unwrap();
        assert_eq!(json, r#""explain""#);
    }

    #[test]
    fn intent_generate_serializes() {
        let json = serde_json::to_string(&CopilotIntent::Generate).unwrap();
        assert_eq!(json, r#""generate""#);
    }

    #[test]
    fn intent_fix_serializes() {
        let json = serde_json::to_string(&CopilotIntent::Fix).unwrap();
        assert_eq!(json, r#""fix""#);
    }

    #[test]
    fn intent_test_serializes() {
        let json = serde_json::to_string(&CopilotIntent::Test).unwrap();
        assert_eq!(json, r#""test""#);
    }

    #[test]
    fn intent_custom_serializes() {
        let intent = CopilotIntent::Custom("document".into());
        let json = serde_json::to_string(&intent).unwrap();
        assert!(json.contains("document"));
    }

    #[test]
    fn intent_roundtrip_serde() {
        for intent in [
            CopilotIntent::Explain,
            CopilotIntent::Generate,
            CopilotIntent::Fix,
            CopilotIntent::Test,
            CopilotIntent::Custom("refactor".into()),
        ] {
            let json = serde_json::to_string(&intent).unwrap();
            let back: CopilotIntent = serde_json::from_str(&json).unwrap();
            assert_eq!(back, intent);
        }
    }

    #[test]
    fn intent_display() {
        assert_eq!(CopilotIntent::Explain.to_string(), "explain");
        assert_eq!(CopilotIntent::Generate.to_string(), "generate");
        assert_eq!(CopilotIntent::Fix.to_string(), "fix");
        assert_eq!(CopilotIntent::Test.to_string(), "test");
        assert_eq!(CopilotIntent::Custom("doc".into()).to_string(), "doc");
    }

    // ── 2. Intent maps to work order vendor ─────────────────────────────

    #[test]
    fn intent_maps_to_work_order() {
        let req = CopilotChatRequest {
            model: "gpt-4o".into(),
            messages: make_messages(&[("user", "explain this code")]),
            tools: None,
            intent: Some(CopilotIntent::Explain),
            doc_context: None,
            references: vec![],
            skills: vec![],
            turn_history: vec![],
            temperature: None,
            max_tokens: None,
            stream: None,
        };

        let wo = copilot_to_work_order(&req);
        let vendor_intent = wo.config.vendor.get("copilot_intent").unwrap();
        assert_eq!(vendor_intent, &serde_json::json!("explain"));
    }

    // ── 3. Code reference preservation ──────────────────────────────────

    #[test]
    fn code_references_in_work_order() {
        let req = CopilotChatRequest {
            model: "gpt-4o".into(),
            messages: make_messages(&[("user", "fix this")]),
            tools: None,
            intent: Some(CopilotIntent::Fix),
            doc_context: None,
            references: vec![
                CopilotCodeReference {
                    path: "src/main.rs".into(),
                    language: Some("rust".into()),
                    selection: None,
                    content: Some("fn main() {}".into()),
                },
                CopilotCodeReference {
                    path: "src/lib.rs".into(),
                    language: Some("rust".into()),
                    selection: Some(SelectionRange {
                        start_line: 10,
                        start_column: 0,
                        end_line: 20,
                        end_column: 0,
                        text: Some("selected code".into()),
                    }),
                    content: None,
                },
            ],
            skills: vec![],
            turn_history: vec![],
            temperature: None,
            max_tokens: None,
            stream: None,
        };

        let wo = copilot_to_work_order(&req);
        // Files extracted from references
        assert!(wo.context.files.contains(&"src/main.rs".to_string()));
        assert!(wo.context.files.contains(&"src/lib.rs".to_string()));
        // Snippet from reference with content
        assert_eq!(wo.context.snippets.len(), 1);
        assert_eq!(wo.context.snippets[0].name, "src/main.rs");
        assert_eq!(wo.context.snippets[0].content, "fn main() {}");
        // References preserved in vendor config
        let vendor_refs = wo.config.vendor.get("copilot_references").unwrap();
        let refs: Vec<CopilotCodeReference> = serde_json::from_value(vendor_refs.clone()).unwrap();
        assert_eq!(refs.len(), 2);
    }

    #[test]
    fn code_ref_to_sdk_preserves_data() {
        let code_ref = CopilotCodeReference {
            path: "src/app.ts".into(),
            language: Some("typescript".into()),
            selection: Some(SelectionRange {
                start_line: 5,
                start_column: 0,
                end_line: 10,
                end_column: 40,
                text: Some("selected".into()),
            }),
            content: Some("const x = 1;".into()),
        };

        let sdk_ref = code_ref_to_sdk(&code_ref);
        assert_eq!(sdk_ref.ref_type, CopilotReferenceType::File);
        assert!(sdk_ref.id.contains("app.ts"));
        assert!(sdk_ref.metadata.is_some());
    }

    // ── 4. Editor context passthrough ───────────────────────────────────

    #[test]
    fn doc_context_in_work_order() {
        let req = CopilotChatRequest {
            model: "gpt-4o".into(),
            messages: make_messages(&[("user", "explain")]),
            tools: None,
            intent: None,
            doc_context: Some(CopilotDocContext {
                uri: "src/editor.rs".into(),
                language: Some("rust".into()),
                cursor_line: Some(42),
                cursor_column: Some(10),
                selection: None,
                content: Some("fn editor() {}".into()),
            }),
            references: vec![],
            skills: vec![],
            turn_history: vec![],
            temperature: None,
            max_tokens: None,
            stream: None,
        };

        let wo = copilot_to_work_order(&req);
        // Doc context file added to context
        assert!(wo.context.files.contains(&"src/editor.rs".to_string()));
        // Doc context content becomes a snippet
        assert_eq!(wo.context.snippets.len(), 1);
        assert_eq!(wo.context.snippets[0].content, "fn editor() {}");
        // Full doc context preserved in vendor
        let doc_ctx_val = wo.config.vendor.get("copilot_doc_context").unwrap();
        let doc_ctx: CopilotDocContext = serde_json::from_value(doc_ctx_val.clone()).unwrap();
        assert_eq!(doc_ctx.cursor_line, Some(42));
    }

    #[test]
    fn doc_context_serde_roundtrip() {
        let ctx = CopilotDocContext {
            uri: "file.py".into(),
            language: Some("python".into()),
            cursor_line: Some(10),
            cursor_column: Some(5),
            selection: Some(SelectionRange {
                start_line: 10,
                start_column: 0,
                end_line: 15,
                end_column: 20,
                text: Some("def foo():".into()),
            }),
            content: None,
        };
        let json = serde_json::to_string(&ctx).unwrap();
        let back: CopilotDocContext = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ctx);
    }

    // ── 5. Skills mapping ───────────────────────────────────────────────

    #[test]
    fn skills_in_work_order() {
        let req = CopilotChatRequest {
            model: "gpt-4o".into(),
            messages: make_messages(&[("user", "use my skill")]),
            tools: None,
            intent: None,
            doc_context: None,
            references: vec![],
            skills: vec![
                CopilotSkill {
                    id: "web-search".into(),
                    name: "Web Search".into(),
                    description: Some("Search the web".into()),
                    parameters_schema: None,
                },
                CopilotSkill {
                    id: "code-review".into(),
                    name: "Code Review".into(),
                    description: None,
                    parameters_schema: Some(serde_json::json!({"type": "object"})),
                },
            ],
            turn_history: vec![],
            temperature: None,
            max_tokens: None,
            stream: None,
        };

        let wo = copilot_to_work_order(&req);
        let skills_val = wo.config.vendor.get("copilot_skills").unwrap();
        let skills: Vec<CopilotSkill> = serde_json::from_value(skills_val.clone()).unwrap();
        assert_eq!(skills.len(), 2);
        assert_eq!(skills[0].id, "web-search");
        assert_eq!(skills[1].id, "code-review");
    }

    #[test]
    fn skill_serde_roundtrip() {
        let skill = CopilotSkill {
            id: "test-skill".into(),
            name: "Test".into(),
            description: Some("A test skill".into()),
            parameters_schema: Some(serde_json::json!({"type": "object", "properties": {}})),
        };
        let json = serde_json::to_string(&skill).unwrap();
        let back: CopilotSkill = serde_json::from_str(&json).unwrap();
        assert_eq!(back, skill);
    }

    // ── 6. Streaming events ─────────────────────────────────────────────

    #[test]
    fn streaming_text_delta() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "hello".into(),
            },
            ext: None,
        };

        let stream_events = agent_event_to_copilot_stream(&event);
        assert_eq!(stream_events.len(), 1);
        match &stream_events[0] {
            CopilotLocalStreamEvent::TextDelta { text } => assert_eq!(text, "hello"),
            other => panic!("expected TextDelta, got {other:?}"),
        }
    }

    #[test]
    fn streaming_assistant_message_becomes_delta() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "full message".into(),
            },
            ext: None,
        };

        let stream_events = agent_event_to_copilot_stream(&event);
        assert_eq!(stream_events.len(), 1);
        assert!(matches!(
            &stream_events[0],
            CopilotLocalStreamEvent::TextDelta { .. }
        ));
    }

    #[test]
    fn streaming_function_call() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "search".into(),
                tool_use_id: Some("call_1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"q": "rust"}),
            },
            ext: None,
        };

        let stream_events = agent_event_to_copilot_stream(&event);
        assert_eq!(stream_events.len(), 1);
        match &stream_events[0] {
            CopilotLocalStreamEvent::FunctionCall { function_call } => {
                assert_eq!(function_call.name, "search");
                assert_eq!(function_call.id.as_deref(), Some("call_1"));
            }
            other => panic!("expected FunctionCall, got {other:?}"),
        }
    }

    #[test]
    fn streaming_error_event() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error {
                message: "rate limit".into(),
                error_code: None,
            },
            ext: None,
        };

        let stream_events = agent_event_to_copilot_stream(&event);
        assert_eq!(stream_events.len(), 1);
        match &stream_events[0] {
            CopilotLocalStreamEvent::CopilotErrors { errors } => {
                assert_eq!(errors.len(), 1);
                assert!(errors[0].message.contains("rate limit"));
            }
            other => panic!("expected CopilotErrors, got {other:?}"),
        }
    }

    #[test]
    fn streaming_run_completed_becomes_done() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        };

        let stream_events = agent_event_to_copilot_stream(&event);
        assert_eq!(stream_events.len(), 1);
        assert!(matches!(
            &stream_events[0],
            CopilotLocalStreamEvent::Done {}
        ));
    }

    #[test]
    fn streaming_unsupported_event_returns_empty() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::FileChanged {
                path: "foo.rs".into(),
                summary: "changed".into(),
            },
            ext: None,
        };

        let stream_events = agent_event_to_copilot_stream(&event);
        assert!(stream_events.is_empty());
    }

    #[test]
    fn receipt_trace_to_stream_bookends() {
        let events = vec![
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantDelta { text: "Hi".into() },
                ext: None,
            },
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantDelta { text: "!".into() },
                ext: None,
            },
        ];

        let stream = receipt_trace_to_copilot_stream(&events, "gpt-4o");
        // references + 2 deltas + metadata + done
        assert_eq!(stream.len(), 5);
        assert!(matches!(
            &stream[0],
            CopilotLocalStreamEvent::CopilotReferences { .. }
        ));
        assert!(matches!(
            &stream[1],
            CopilotLocalStreamEvent::TextDelta { .. }
        ));
        assert!(matches!(
            &stream[2],
            CopilotLocalStreamEvent::TextDelta { .. }
        ));
        assert!(matches!(
            &stream[3],
            CopilotLocalStreamEvent::Metadata { .. }
        ));
        assert!(matches!(&stream[4], CopilotLocalStreamEvent::Done {}));
    }

    // ── 7. Round-trip translation ───────────────────────────────────────

    #[test]
    fn roundtrip_request_to_work_order_to_response() {
        let req = CopilotChatRequest {
            model: "gpt-4o".into(),
            messages: make_messages(&[("system", "You are helpful"), ("user", "Say hello")]),
            tools: None,
            intent: Some(CopilotIntent::Generate),
            doc_context: Some(CopilotDocContext {
                uri: "test.rs".into(),
                language: Some("rust".into()),
                cursor_line: Some(1),
                cursor_column: Some(0),
                selection: None,
                content: None,
            }),
            references: vec![CopilotCodeReference {
                path: "lib.rs".into(),
                language: Some("rust".into()),
                selection: None,
                content: Some("pub fn hello() {}".into()),
            }],
            skills: vec![CopilotSkill {
                id: "gen".into(),
                name: "Generator".into(),
                description: None,
                parameters_schema: None,
            }],
            turn_history: vec![],
            temperature: Some(0.7),
            max_tokens: Some(1000),
            stream: None,
        };

        // Request → WorkOrder
        let wo = copilot_to_work_order(&req);
        assert_eq!(wo.task, "Say hello");
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
        assert!(wo.config.vendor.contains_key("copilot_intent"));
        assert!(wo.config.vendor.contains_key("copilot_doc_context"));
        assert!(wo.config.vendor.contains_key("copilot_references"));
        assert!(wo.config.vendor.contains_key("copilot_skills"));
        assert_eq!(
            wo.config.vendor.get("temperature"),
            Some(&serde_json::json!(0.7))
        );
        assert_eq!(
            wo.config.vendor.get("max_tokens"),
            Some(&serde_json::json!(1000))
        );

        // Simulate receipt
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Hello, world!".into(),
            },
            ext: None,
        }];
        let receipt = make_receipt(events);

        // Receipt → CopilotChatResponse
        let resp = receipt_to_copilot(&receipt, "gpt-4o");
        assert_eq!(resp.message, "Hello, world!");
        assert!(resp.copilot_errors.is_empty());
        assert!(resp.metadata.is_some());
        assert_eq!(
            resp.metadata.as_ref().unwrap().model.as_deref(),
            Some("gpt-4o")
        );
    }

    // ── 8. Error handling ───────────────────────────────────────────────

    #[test]
    fn empty_messages_defaults_task() {
        let req = CopilotChatRequest {
            model: "gpt-4o".into(),
            messages: vec![],
            tools: None,
            intent: None,
            doc_context: None,
            references: vec![],
            skills: vec![],
            turn_history: vec![],
            temperature: None,
            max_tokens: None,
            stream: None,
        };

        let wo = copilot_to_work_order(&req);
        assert_eq!(wo.task, "copilot completion");
    }

    #[test]
    fn empty_messages_with_intent_uses_intent_as_task() {
        let req = CopilotChatRequest {
            model: "gpt-4o".into(),
            messages: vec![],
            tools: None,
            intent: Some(CopilotIntent::Fix),
            doc_context: None,
            references: vec![],
            skills: vec![],
            turn_history: vec![],
            temperature: None,
            max_tokens: None,
            stream: None,
        };

        let wo = copilot_to_work_order(&req);
        assert_eq!(wo.task, "copilot fix");
    }

    #[test]
    fn error_events_in_receipt() {
        let events = vec![
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::Error {
                    message: "timeout".into(),
                    error_code: None,
                },
                ext: None,
            },
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::Error {
                    message: "rate limit".into(),
                    error_code: None,
                },
                ext: None,
            },
        ];
        let receipt = make_receipt(events);
        let resp = receipt_to_copilot(&receipt, "gpt-4o");
        assert_eq!(resp.copilot_errors.len(), 2);
        assert!(resp.copilot_errors[0].message.contains("timeout"));
        assert!(resp.copilot_errors[1].message.contains("rate limit"));
    }

    #[test]
    fn function_call_in_receipt() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("call_xyz".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "main.rs"}),
            },
            ext: None,
        }];
        let receipt = make_receipt(events);
        let resp = receipt_to_copilot(&receipt, "gpt-4o");
        let fc = resp.function_call.unwrap();
        assert_eq!(fc.name, "read_file");
        assert_eq!(fc.id.as_deref(), Some("call_xyz"));
    }

    // ── 9. CopilotChatRequest serde ─────────────────────────────────────

    #[test]
    fn chat_request_serde_roundtrip() {
        let req = CopilotChatRequest {
            model: "gpt-4o".into(),
            messages: make_messages(&[("user", "hello")]),
            tools: None,
            intent: Some(CopilotIntent::Explain),
            doc_context: Some(CopilotDocContext {
                uri: "test.py".into(),
                language: Some("python".into()),
                cursor_line: Some(0),
                cursor_column: Some(0),
                selection: None,
                content: None,
            }),
            references: vec![CopilotCodeReference {
                path: "test.py".into(),
                language: Some("python".into()),
                selection: None,
                content: None,
            }],
            skills: vec![],
            turn_history: vec![],
            temperature: Some(0.5),
            max_tokens: Some(2048),
            stream: Some(true),
        };

        let json = serde_json::to_string(&req).unwrap();
        let back: CopilotChatRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.model, "gpt-4o");
        assert_eq!(back.intent, Some(CopilotIntent::Explain));
        assert_eq!(back.temperature, Some(0.5));
        assert_eq!(back.max_tokens, Some(2048));
        assert_eq!(back.stream, Some(true));
    }

    // ── 10. CopilotChatResponse serde ───────────────────────────────────

    #[test]
    fn chat_response_serde_roundtrip() {
        let resp = CopilotChatResponse {
            message: "Hello!".into(),
            copilot_references: vec![],
            copilot_errors: vec![],
            function_call: None,
            metadata: Some(CopilotResponseMetadata {
                intent: Some(CopilotIntent::Explain),
                model: Some("gpt-4o".into()),
                ext: BTreeMap::new(),
            }),
        };

        let json = serde_json::to_string(&resp).unwrap();
        let back: CopilotChatResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.message, "Hello!");
        assert_eq!(back.metadata.unwrap().intent, Some(CopilotIntent::Explain));
    }

    // ── 11. Optional fields skip serialization ──────────────────────────

    #[test]
    fn optional_fields_omitted_when_none() {
        let req = CopilotChatRequest {
            model: "gpt-4o".into(),
            messages: make_messages(&[("user", "hi")]),
            tools: None,
            intent: None,
            doc_context: None,
            references: vec![],
            skills: vec![],
            turn_history: vec![],
            temperature: None,
            max_tokens: None,
            stream: None,
        };

        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.contains("intent"));
        assert!(!json.contains("doc_context"));
        assert!(!json.contains("temperature"));
        assert!(!json.contains("max_tokens"));
        assert!(!json.contains("stream"));
    }

    // ── 12. Deduplication of doc_context file in context ────────────────

    #[test]
    fn doc_context_file_not_duplicated() {
        let req = CopilotChatRequest {
            model: "gpt-4o".into(),
            messages: make_messages(&[("user", "check")]),
            tools: None,
            intent: None,
            doc_context: Some(CopilotDocContext {
                uri: "src/lib.rs".into(),
                language: None,
                cursor_line: None,
                cursor_column: None,
                selection: None,
                content: None,
            }),
            references: vec![CopilotCodeReference {
                path: "src/lib.rs".into(),
                language: None,
                selection: None,
                content: None,
            }],
            skills: vec![],
            turn_history: vec![],
            temperature: None,
            max_tokens: None,
            stream: None,
        };

        let wo = copilot_to_work_order(&req);
        // Should appear only once even though both doc_context and references have it
        let count = wo
            .context
            .files
            .iter()
            .filter(|f| *f == "src/lib.rs")
            .count();
        assert_eq!(count, 1);
    }

    // ── 13. CopilotLocalStreamEvent serde ───────────────────────────────

    #[test]
    fn stream_event_serde_roundtrip() {
        let events = vec![
            CopilotLocalStreamEvent::CopilotReferences { references: vec![] },
            CopilotLocalStreamEvent::TextDelta {
                text: "hello".into(),
            },
            CopilotLocalStreamEvent::CopilotErrors {
                errors: vec![CopilotError {
                    error_type: "test_error".into(),
                    message: "boom".into(),
                    code: None,
                    identifier: None,
                }],
            },
            CopilotLocalStreamEvent::Metadata {
                metadata: CopilotResponseMetadata {
                    intent: None,
                    model: Some("gpt-4o".into()),
                    ext: BTreeMap::new(),
                },
            },
            CopilotLocalStreamEvent::Done {},
        ];

        for event in &events {
            let json = serde_json::to_string(event).unwrap();
            let back: CopilotLocalStreamEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, event);
        }
    }

    // ── 14. Dialect marker ──────────────────────────────────────────────

    #[test]
    fn dialect_marker_set_in_vendor() {
        let req = CopilotChatRequest {
            model: "gpt-4o".into(),
            messages: make_messages(&[("user", "test")]),
            tools: None,
            intent: None,
            doc_context: None,
            references: vec![],
            skills: vec![],
            turn_history: vec![],
            temperature: None,
            max_tokens: None,
            stream: None,
        };

        let wo = copilot_to_work_order(&req);
        assert_eq!(
            wo.config.vendor.get("dialect"),
            Some(&serde_json::json!("copilot"))
        );
    }

    // ── 15. SelectionRange serde ────────────────────────────────────────

    #[test]
    fn selection_range_serde_roundtrip() {
        let range = SelectionRange {
            start_line: 1,
            start_column: 5,
            end_line: 10,
            end_column: 20,
            text: Some("selected text".into()),
        };
        let json = serde_json::to_string(&range).unwrap();
        let back: SelectionRange = serde_json::from_str(&json).unwrap();
        assert_eq!(back, range);
    }

    // ── 16. Empty receipt produces empty response ───────────────────────

    #[test]
    fn empty_receipt_produces_empty_response() {
        let receipt = make_receipt(vec![]);
        let resp = receipt_to_copilot(&receipt, "gpt-4o");
        assert!(resp.message.is_empty());
        assert!(resp.copilot_errors.is_empty());
        assert!(resp.function_call.is_none());
    }

    // ── 17. Model propagation ───────────────────────────────────────────

    #[test]
    fn model_propagated_to_work_order() {
        let req = CopilotChatRequest {
            model: "o3-mini".into(),
            messages: make_messages(&[("user", "test")]),
            tools: None,
            intent: None,
            doc_context: None,
            references: vec![],
            skills: vec![],
            turn_history: vec![],
            temperature: None,
            max_tokens: None,
            stream: None,
        };

        let wo = copilot_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("o3-mini"));
    }
}
