// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for all output formats and contract types.

use abp_core::{
    AgentEvent, AgentEventKind, ExecutionLane, Outcome, ReceiptBuilder, WorkOrderBuilder,
};
use abp_format::{Formatter, OutputFormat};
use chrono::Utc;

fn sample_receipt() -> abp_core::Receipt {
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "starting".into(),
        },
        ext: None,
    };
    ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_trace_event(ev)
        .build()
}

fn sample_event() -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "Read".into(),
            tool_use_id: Some("t1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"path": "src/main.rs"}),
        },
        ext: None,
    }
}

fn sample_work_order() -> abp_core::WorkOrder {
    WorkOrderBuilder::new("Fix the login bug in the authentication module")
        .lane(ExecutionLane::PatchFirst)
        .root("/tmp/ws")
        .model("gpt-4")
        .build()
}

#[test]
fn receipt_json_is_valid() {
    let f = Formatter::new(OutputFormat::Json);
    let out = f.format_receipt(&sample_receipt());
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    assert_eq!(v["outcome"], "complete");
}

#[test]
fn receipt_json_pretty_is_multiline() {
    let f = Formatter::new(OutputFormat::JsonPretty);
    let out = f.format_receipt(&sample_receipt());
    assert!(out.contains('\n'));
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    assert_eq!(v["outcome"], "complete");
}

#[test]
fn event_json_is_valid() {
    let f = Formatter::new(OutputFormat::Json);
    let out = f.format_event(&sample_event());
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    assert_eq!(v["type"], "tool_call");
}

#[test]
fn work_order_compact_single_line() {
    let f = Formatter::new(OutputFormat::Compact);
    let wo = sample_work_order();
    let out = f.format_work_order(&wo);
    assert!(!out.contains('\n'));
    assert!(out.contains("lane=patch_first"));
}

#[test]
fn output_format_rejects_unknown() {
    assert!("xml".parse::<OutputFormat>().is_err());
}
