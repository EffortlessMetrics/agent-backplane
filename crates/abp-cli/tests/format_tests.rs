// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for the `format` module covering all output formats and contract types.

use abp_cli::format::{Formatter, OutputFormat};
use abp_core::{
    AgentEvent, AgentEventKind, ExecutionLane, Outcome, ReceiptBuilder, WorkOrderBuilder,
};
use chrono::Utc;

// ── Helpers ───────────────────────────────────────────────────────────

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

// ── Receipt tests ─────────────────────────────────────────────────────

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
    assert!(out.contains('\n'), "pretty JSON should be multiline");
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    assert_eq!(v["outcome"], "complete");
}

#[test]
fn receipt_text_contains_outcome_and_duration() {
    let f = Formatter::new(OutputFormat::Text);
    let out = f.format_receipt(&sample_receipt());
    assert!(out.contains("Outcome: complete"));
    assert!(out.contains("Duration:"));
    assert!(out.contains("Events: 1"));
}

#[test]
fn receipt_table_has_aligned_keys() {
    let f = Formatter::new(OutputFormat::Table);
    let out = f.format_receipt(&sample_receipt());
    assert!(out.contains("outcome"));
    assert!(out.contains("backend"));
    assert!(out.contains("duration"));
    assert!(out.contains("run_id"));
}

#[test]
fn receipt_compact_single_line() {
    let f = Formatter::new(OutputFormat::Compact);
    let out = f.format_receipt(&sample_receipt());
    assert!(!out.contains('\n'), "compact should be single line");
    assert!(out.contains("[complete]"));
    assert!(out.contains("backend=mock"));
}

// ── Event tests ───────────────────────────────────────────────────────

#[test]
fn event_json_is_valid() {
    let f = Formatter::new(OutputFormat::Json);
    let out = f.format_event(&sample_event());
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    assert_eq!(v["type"], "tool_call");
    assert_eq!(v["tool_name"], "Read");
}

#[test]
fn event_text_shows_timestamp_and_kind() {
    let f = Formatter::new(OutputFormat::Text);
    let out = f.format_event(&sample_event());
    assert!(out.contains("tool_call"));
    assert!(out.contains("call Read"));
}

#[test]
fn event_table_shows_columns() {
    let f = Formatter::new(OutputFormat::Table);
    let out = f.format_event(&sample_event());
    assert!(out.contains("tool_call"));
    assert!(out.contains("call Read"));
}

#[test]
fn event_compact_single_line() {
    let f = Formatter::new(OutputFormat::Compact);
    let out = f.format_event(&sample_event());
    assert!(!out.contains('\n'));
    assert!(out.contains("[tool_call]"));
}

// ── WorkOrder tests ───────────────────────────────────────────────────

#[test]
fn work_order_text_shows_id_and_task() {
    let f = Formatter::new(OutputFormat::Text);
    let wo = sample_work_order();
    let out = f.format_work_order(&wo);
    assert!(out.contains("ID:"));
    assert!(out.contains("Fix the login bug"));
    assert!(out.contains("Lane: patch_first"));
}

#[test]
fn work_order_table_shows_model() {
    let f = Formatter::new(OutputFormat::Table);
    let wo = sample_work_order();
    let out = f.format_work_order(&wo);
    assert!(out.contains("gpt-4"));
    assert!(out.contains("model"));
}

#[test]
fn work_order_compact_single_line() {
    let f = Formatter::new(OutputFormat::Compact);
    let wo = sample_work_order();
    let out = f.format_work_order(&wo);
    assert!(!out.contains('\n'));
    assert!(out.contains("lane=patch_first"));
}

// ── Error formatting ──────────────────────────────────────────────────

#[test]
fn error_json_wraps_message() {
    let f = Formatter::new(OutputFormat::Json);
    let out = f.format_error("something broke");
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    assert_eq!(v["error"], "something broke");
}

#[test]
fn error_text_prefixed() {
    let f = Formatter::new(OutputFormat::Text);
    let out = f.format_error("something broke");
    assert!(out.starts_with("Error: "));
}

#[test]
fn error_compact_bracketed() {
    let f = Formatter::new(OutputFormat::Compact);
    let out = f.format_error("oops");
    assert_eq!(out, "[error] oops");
}

// ── OutputFormat parsing ──────────────────────────────────────────────

#[test]
fn output_format_roundtrip() {
    for fmt in &[
        OutputFormat::Json,
        OutputFormat::JsonPretty,
        OutputFormat::Text,
        OutputFormat::Table,
        OutputFormat::Compact,
    ] {
        let s = fmt.to_string();
        let parsed: OutputFormat = s.parse().unwrap();
        assert_eq!(&parsed, fmt);
    }
}

#[test]
fn output_format_rejects_unknown() {
    assert!("xml".parse::<OutputFormat>().is_err());
}
