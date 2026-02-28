// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for [`abp_core::aggregate`].

use std::collections::BTreeMap;

use abp_core::aggregate::{AggregationSummary, EventAggregator, RunAnalytics};
use abp_core::{AgentEvent, AgentEventKind};
use chrono::{Duration as ChronoDuration, Utc};

fn event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn sample_events() -> Vec<AgentEvent> {
    let base = Utc::now();
    let ts = |ms: i64| base + ChronoDuration::milliseconds(ms);
    vec![
        AgentEvent { ts: ts(0), kind: AgentEventKind::RunStarted { message: "go".into() }, ext: None },
        AgentEvent { ts: ts(100), kind: AgentEventKind::AssistantMessage { text: "hello".into() }, ext: None },
        AgentEvent { ts: ts(200), kind: AgentEventKind::ToolCall { tool_name: "grep".into(), tool_use_id: None, parent_tool_use_id: None, input: serde_json::json!({}) }, ext: None },
        AgentEvent { ts: ts(300), kind: AgentEventKind::ToolResult { tool_name: "grep".into(), tool_use_id: None, output: serde_json::json!("result"), is_error: false }, ext: None },
        AgentEvent { ts: ts(400), kind: AgentEventKind::ToolCall { tool_name: "edit".into(), tool_use_id: None, parent_tool_use_id: None, input: serde_json::json!({}) }, ext: None },
        AgentEvent { ts: ts(500), kind: AgentEventKind::ToolResult { tool_name: "edit".into(), tool_use_id: None, output: serde_json::json!("ok"), is_error: false }, ext: None },
        AgentEvent { ts: ts(600), kind: AgentEventKind::AssistantMessage { text: "done editing".into() }, ext: None },
        AgentEvent { ts: ts(700), kind: AgentEventKind::RunCompleted { message: "finished".into() }, ext: None },
    ]
}

// ── EventAggregator basic ────────────────────────────────────────

#[test]
fn new_aggregator_is_empty() {
    let agg = EventAggregator::new();
    assert_eq!(agg.event_count(), 0);
}

#[test]
fn default_aggregator_is_empty() {
    let agg = EventAggregator::default();
    assert_eq!(agg.event_count(), 0);
}

#[test]
fn add_increments_count() {
    let mut agg = EventAggregator::new();
    agg.add(&event(AgentEventKind::RunStarted { message: "go".into() }));
    assert_eq!(agg.event_count(), 1);
    agg.add(&event(AgentEventKind::RunCompleted { message: "done".into() }));
    assert_eq!(agg.event_count(), 2);
}

#[test]
fn count_by_kind_groups_correctly() {
    let mut agg = EventAggregator::new();
    agg.add(&event(AgentEventKind::RunStarted { message: "a".into() }));
    agg.add(&event(AgentEventKind::Warning { message: "w1".into() }));
    agg.add(&event(AgentEventKind::Warning { message: "w2".into() }));
    let counts = agg.count_by_kind();
    assert_eq!(counts.get("run_started"), Some(&1));
    assert_eq!(counts.get("warning"), Some(&2));
}

#[test]
fn count_by_kind_empty() {
    let agg = EventAggregator::new();
    assert!(agg.count_by_kind().is_empty());
}

// ── Timestamps & duration ────────────────────────────────────────

#[test]
fn first_timestamp_none_when_empty() {
    let agg = EventAggregator::new();
    assert!(agg.first_timestamp().is_none());
}

#[test]
fn last_timestamp_none_when_empty() {
    let agg = EventAggregator::new();
    assert!(agg.last_timestamp().is_none());
}

#[test]
fn timestamps_present_after_add() {
    let mut agg = EventAggregator::new();
    agg.add(&event(AgentEventKind::RunStarted { message: "x".into() }));
    assert!(agg.first_timestamp().is_some());
    assert!(agg.last_timestamp().is_some());
}

#[test]
fn duration_ms_none_with_zero_events() {
    let agg = EventAggregator::new();
    assert!(agg.duration_ms().is_none());
}

#[test]
fn duration_ms_none_with_one_event() {
    let mut agg = EventAggregator::new();
    agg.add(&event(AgentEventKind::RunStarted { message: "x".into() }));
    assert!(agg.duration_ms().is_none());
}

#[test]
fn duration_ms_computed_from_events() {
    let base = Utc::now();
    let mut agg = EventAggregator::new();
    agg.add(&AgentEvent { ts: base, kind: AgentEventKind::RunStarted { message: "go".into() }, ext: None });
    agg.add(&AgentEvent { ts: base + ChronoDuration::milliseconds(500), kind: AgentEventKind::RunCompleted { message: "done".into() }, ext: None });
    assert_eq!(agg.duration_ms(), Some(500));
}

// ── Tool calls ───────────────────────────────────────────────────

#[test]
fn tool_calls_empty_when_no_tool_events() {
    let mut agg = EventAggregator::new();
    agg.add(&event(AgentEventKind::RunStarted { message: "go".into() }));
    assert!(agg.tool_calls().is_empty());
}

#[test]
fn tool_calls_returns_names_in_order() {
    let mut agg = EventAggregator::new();
    agg.add(&event(AgentEventKind::ToolCall { tool_name: "grep".into(), tool_use_id: None, parent_tool_use_id: None, input: serde_json::json!({}) }));
    agg.add(&event(AgentEventKind::ToolCall { tool_name: "edit".into(), tool_use_id: None, parent_tool_use_id: None, input: serde_json::json!({}) }));
    assert_eq!(agg.tool_calls(), vec!["grep", "edit"]);
}

#[test]
fn unique_tool_count_deduplicates() {
    let mut agg = EventAggregator::new();
    agg.add(&event(AgentEventKind::ToolCall { tool_name: "grep".into(), tool_use_id: None, parent_tool_use_id: None, input: serde_json::json!({}) }));
    agg.add(&event(AgentEventKind::ToolCall { tool_name: "grep".into(), tool_use_id: None, parent_tool_use_id: None, input: serde_json::json!({}) }));
    agg.add(&event(AgentEventKind::ToolCall { tool_name: "edit".into(), tool_use_id: None, parent_tool_use_id: None, input: serde_json::json!({}) }));
    assert_eq!(agg.unique_tool_count(), 2);
}

#[test]
fn unique_tool_count_zero_when_empty() {
    let agg = EventAggregator::new();
    assert_eq!(agg.unique_tool_count(), 0);
}

// ── Errors ───────────────────────────────────────────────────────

#[test]
fn has_errors_false_when_no_errors() {
    let mut agg = EventAggregator::new();
    agg.add(&event(AgentEventKind::RunStarted { message: "go".into() }));
    assert!(!agg.has_errors());
}

#[test]
fn has_errors_true_when_error_present() {
    let mut agg = EventAggregator::new();
    agg.add(&event(AgentEventKind::Error { message: "boom".into() }));
    assert!(agg.has_errors());
}

#[test]
fn error_messages_collected() {
    let mut agg = EventAggregator::new();
    agg.add(&event(AgentEventKind::Error { message: "e1".into() }));
    agg.add(&event(AgentEventKind::Warning { message: "w".into() }));
    agg.add(&event(AgentEventKind::Error { message: "e2".into() }));
    assert_eq!(agg.error_messages(), vec!["e1", "e2"]);
}

#[test]
fn error_messages_empty_when_none() {
    let agg = EventAggregator::new();
    assert!(agg.error_messages().is_empty());
}

// ── Text length ──────────────────────────────────────────────────

#[test]
fn text_length_sums_assistant_text() {
    let mut agg = EventAggregator::new();
    agg.add(&event(AgentEventKind::AssistantMessage { text: "hello".into() }));        // 5
    agg.add(&event(AgentEventKind::AssistantDelta { text: "wor".into() }));             // 3
    agg.add(&event(AgentEventKind::ToolCall { tool_name: "x".into(), tool_use_id: None, parent_tool_use_id: None, input: serde_json::json!({}) })); // 0
    assert_eq!(agg.text_length(), 8);
}

#[test]
fn text_length_zero_when_empty() {
    let agg = EventAggregator::new();
    assert_eq!(agg.text_length(), 0);
}

// ── AggregationSummary ──────────────────────────────────────────

#[test]
fn summary_from_sample_events() {
    let events = sample_events();
    let mut agg = EventAggregator::new();
    for e in &events {
        agg.add(e);
    }
    let s = agg.summary();
    assert_eq!(s.total_events, 8);
    assert_eq!(s.tool_calls, 2);
    assert_eq!(s.unique_tools, 2);
    assert_eq!(s.errors, 0);
    // "hello" (5) + "done editing" (12) = 17
    assert_eq!(s.total_text_chars, 17);
    assert!(s.duration_ms.is_some());
}

#[test]
fn summary_serializes_to_json() {
    let s = AggregationSummary {
        total_events: 1,
        by_kind: BTreeMap::new(),
        tool_calls: 0,
        unique_tools: 0,
        errors: 0,
        total_text_chars: 0,
        duration_ms: None,
    };
    let json = serde_json::to_string(&s).unwrap();
    let deser: AggregationSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(s, deser);
}

// ── RunAnalytics ─────────────────────────────────────────────────

#[test]
fn run_analytics_successful_without_errors() {
    let events = sample_events();
    let analytics = RunAnalytics::from_events(&events);
    assert!(analytics.is_successful());
}

#[test]
fn run_analytics_not_successful_with_errors() {
    let events = vec![
        event(AgentEventKind::RunStarted { message: "go".into() }),
        event(AgentEventKind::Error { message: "oops".into() }),
    ];
    let analytics = RunAnalytics::from_events(&events);
    assert!(!analytics.is_successful());
}

#[test]
fn tool_usage_ratio_computed() {
    let events = sample_events(); // 8 events, 2 tool calls
    let analytics = RunAnalytics::from_events(&events);
    let ratio = analytics.tool_usage_ratio();
    assert!((ratio - 0.25).abs() < f64::EPSILON);
}

#[test]
fn tool_usage_ratio_zero_when_empty() {
    let analytics = RunAnalytics::from_events(&[]);
    assert!((analytics.tool_usage_ratio() - 0.0).abs() < f64::EPSILON);
}

#[test]
fn average_text_per_event_computed() {
    let events = sample_events(); // 8 events, 17 chars
    let analytics = RunAnalytics::from_events(&events);
    let avg = analytics.average_text_per_event();
    assert!((avg - 17.0 / 8.0).abs() < f64::EPSILON);
}

#[test]
fn average_text_per_event_zero_when_empty() {
    let analytics = RunAnalytics::from_events(&[]);
    assert!((analytics.average_text_per_event() - 0.0).abs() < f64::EPSILON);
}

#[test]
fn run_analytics_summary_matches_aggregator() {
    let events = sample_events();
    let analytics = RunAnalytics::from_events(&events);
    let mut agg = EventAggregator::new();
    for e in &events {
        agg.add(e);
    }
    // They should produce identical summaries (apart from duration which
    // depends on exact timing, so compare field-by-field).
    let sa = analytics.summary();
    let sb = agg.summary();
    assert_eq!(sa.total_events, sb.total_events);
    assert_eq!(sa.tool_calls, sb.tool_calls);
    assert_eq!(sa.unique_tools, sb.unique_tools);
    assert_eq!(sa.errors, sb.errors);
    assert_eq!(sa.total_text_chars, sb.total_text_chars);
}
