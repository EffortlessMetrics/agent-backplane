// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for the extension traits in `abp_core::ext`.

use abp_core::ext::{AgentEventExt, ReceiptExt, WorkOrderExt};
use abp_core::{
    AgentEvent, AgentEventKind, Capability, CapabilityRequirement, CapabilityRequirements,
    MinSupport, Outcome, ReceiptBuilder, WorkOrderBuilder,
};
use chrono::Utc;

// ─── helpers ────────────────────────────────────────────────────────────────

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn tool_call_event(name: &str) -> AgentEvent {
    make_event(AgentEventKind::ToolCall {
        tool_name: name.into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: serde_json::json!({}),
    })
}

fn assistant_msg(text: &str) -> AgentEvent {
    make_event(AgentEventKind::AssistantMessage { text: text.into() })
}

fn error_event(msg: &str) -> AgentEvent {
    make_event(AgentEventKind::Error {
        message: msg.into(),
    })
}

// ─── WorkOrderExt ───────────────────────────────────────────────────────────

#[test]
fn has_capability_present() {
    let wo = WorkOrderBuilder::new("task")
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::ToolEdit,
                min_support: MinSupport::Native,
            }],
        })
        .build();
    assert!(wo.has_capability(&Capability::ToolEdit));
}

#[test]
fn has_capability_absent() {
    let wo = WorkOrderBuilder::new("task").build();
    assert!(!wo.has_capability(&Capability::ToolBash));
}

#[test]
fn tool_budget_remaining_some() {
    let wo = WorkOrderBuilder::new("task").max_turns(5).build();
    assert_eq!(wo.tool_budget_remaining(), Some(5));
}

#[test]
fn tool_budget_remaining_none() {
    let wo = WorkOrderBuilder::new("task").build();
    assert_eq!(wo.tool_budget_remaining(), None);
}

#[test]
fn is_code_task_positive() {
    for kw in &["fix the bug", "implement feature", "refactor module", "write code"] {
        let wo = WorkOrderBuilder::new(*kw).build();
        assert!(wo.is_code_task(), "expected true for {kw:?}");
    }
}

#[test]
fn is_code_task_negative() {
    let wo = WorkOrderBuilder::new("write a poem about cats").build();
    assert!(!wo.is_code_task());
}

#[test]
fn is_code_task_case_insensitive() {
    let wo = WorkOrderBuilder::new("IMPLEMENT a new feature").build();
    assert!(wo.is_code_task());
}

#[test]
fn task_summary_short() {
    let wo = WorkOrderBuilder::new("hi").build();
    assert_eq!(wo.task_summary(10), "hi");
}

#[test]
fn task_summary_truncated() {
    let wo = WorkOrderBuilder::new("implement the login feature").build();
    let s = wo.task_summary(10);
    assert!(s.ends_with('…'));
    // The visible part before the ellipsis should be at most 10 bytes.
    assert!(s.len() <= 10 + '…'.len_utf8());
}

#[test]
fn task_summary_exact_boundary() {
    let wo = WorkOrderBuilder::new("abcde").build();
    assert_eq!(wo.task_summary(5), "abcde");
}

#[test]
fn required_capabilities_from_requirements() {
    let wo = WorkOrderBuilder::new("task")
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        })
        .build();
    assert!(wo.required_capabilities().contains(&Capability::Streaming));
}

#[test]
fn required_capabilities_inferred_from_task() {
    let wo = WorkOrderBuilder::new("edit and refactor the module").build();
    let caps = wo.required_capabilities();
    assert!(caps.contains(&Capability::ToolEdit));
}

#[test]
fn required_capabilities_inferred_bash() {
    let wo = WorkOrderBuilder::new("run a shell command").build();
    let caps = wo.required_capabilities();
    assert!(caps.contains(&Capability::ToolBash));
}

#[test]
fn vendor_config_present() {
    let mut wo = WorkOrderBuilder::new("task").build();
    wo.config
        .vendor
        .insert("abp".into(), serde_json::json!({"mode": "passthrough"}));
    assert!(wo.vendor_config("abp").is_some());
}

#[test]
fn vendor_config_absent() {
    let wo = WorkOrderBuilder::new("task").build();
    assert!(wo.vendor_config("missing").is_none());
}

// ─── ReceiptExt ─────────────────────────────────────────────────────────────

#[test]
fn is_success_complete() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    assert!(r.is_success());
    assert!(!r.is_failure());
}

#[test]
fn is_failure_failed() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Failed)
        .build();
    assert!(r.is_failure());
    assert!(!r.is_success());
}

#[test]
fn is_partial_neither() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Partial)
        .build();
    assert!(!r.is_success());
    assert!(!r.is_failure());
}

#[test]
fn event_count_by_kind_empty() {
    let r = ReceiptBuilder::new("mock").build();
    assert!(r.event_count_by_kind().is_empty());
}

#[test]
fn event_count_by_kind_mixed() {
    let r = ReceiptBuilder::new("mock")
        .add_trace_event(tool_call_event("read"))
        .add_trace_event(tool_call_event("write"))
        .add_trace_event(assistant_msg("hello"))
        .add_trace_event(error_event("boom"))
        .build();
    let counts = r.event_count_by_kind();
    assert_eq!(counts.get("tool_call"), Some(&2));
    assert_eq!(counts.get("assistant_message"), Some(&1));
    assert_eq!(counts.get("error"), Some(&1));
}

#[test]
fn tool_calls_filter() {
    let r = ReceiptBuilder::new("mock")
        .add_trace_event(tool_call_event("read"))
        .add_trace_event(assistant_msg("hi"))
        .add_trace_event(tool_call_event("write"))
        .build();
    assert_eq!(r.tool_calls().len(), 2);
}

#[test]
fn assistant_messages_filter() {
    let r = ReceiptBuilder::new("mock")
        .add_trace_event(assistant_msg("a"))
        .add_trace_event(assistant_msg("b"))
        .add_trace_event(tool_call_event("x"))
        .build();
    assert_eq!(r.assistant_messages().len(), 2);
}

#[test]
fn total_tool_calls_count() {
    let r = ReceiptBuilder::new("mock")
        .add_trace_event(tool_call_event("a"))
        .add_trace_event(tool_call_event("b"))
        .add_trace_event(tool_call_event("c"))
        .build();
    assert_eq!(r.total_tool_calls(), 3);
}

#[test]
fn has_errors_true() {
    let r = ReceiptBuilder::new("mock")
        .add_trace_event(error_event("oops"))
        .build();
    assert!(r.has_errors());
}

#[test]
fn has_errors_false() {
    let r = ReceiptBuilder::new("mock")
        .add_trace_event(assistant_msg("ok"))
        .build();
    assert!(!r.has_errors());
}

#[test]
fn duration_secs_conversion() {
    let start = Utc::now();
    let end = start + chrono::Duration::milliseconds(2500);
    let r = ReceiptBuilder::new("mock")
        .started_at(start)
        .finished_at(end)
        .build();
    assert!((r.duration_secs() - 2.5).abs() < 0.01);
}

// ─── AgentEventExt ──────────────────────────────────────────────────────────

#[test]
fn is_tool_call_true() {
    let ev = tool_call_event("read");
    assert!(ev.is_tool_call());
}

#[test]
fn is_tool_call_false() {
    let ev = assistant_msg("hi");
    assert!(!ev.is_tool_call());
}

#[test]
fn is_terminal_run_completed() {
    let ev = make_event(AgentEventKind::RunCompleted {
        message: "done".into(),
    });
    assert!(ev.is_terminal());
}

#[test]
fn is_terminal_other() {
    let ev = make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    assert!(!ev.is_terminal());
}

#[test]
fn text_content_delta() {
    let ev = make_event(AgentEventKind::AssistantDelta {
        text: "chunk".into(),
    });
    assert_eq!(ev.text_content(), Some("chunk"));
}

#[test]
fn text_content_message() {
    let ev = assistant_msg("full");
    assert_eq!(ev.text_content(), Some("full"));
}

#[test]
fn text_content_none_for_tool() {
    let ev = tool_call_event("x");
    assert_eq!(ev.text_content(), None);
}
