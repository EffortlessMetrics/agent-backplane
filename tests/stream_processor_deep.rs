// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep tests for `abp-stream` stream processor: filters, transforms,
//! recorders, stats, pipeline composition, builder pattern, concurrency,
//! and edge cases.

use abp_core::{AgentEvent, AgentEventKind};
use abp_stream::{
    EventFilter, EventMultiplexer, EventRecorder, EventStats, EventStream, EventTransform,
    StreamPipeline, StreamPipelineBuilder, event_kind_name,
};
use chrono::Utc;
use std::collections::BTreeMap;
use tokio::sync::mpsc;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn make_event_with_ext(
    kind: AgentEventKind,
    ext: BTreeMap<String, serde_json::Value>,
) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: Some(ext),
    }
}

fn delta(text: &str) -> AgentEvent {
    make_event(AgentEventKind::AssistantDelta {
        text: text.to_string(),
    })
}

fn message(text: &str) -> AgentEvent {
    make_event(AgentEventKind::AssistantMessage {
        text: text.to_string(),
    })
}

fn error(msg: &str) -> AgentEvent {
    make_event(AgentEventKind::Error {
        message: msg.to_string(),
        error_code: None,
    })
}

fn error_with_code(msg: &str, code: abp_error::ErrorCode) -> AgentEvent {
    make_event(AgentEventKind::Error {
        message: msg.to_string(),
        error_code: Some(code),
    })
}

fn tool_call(name: &str) -> AgentEvent {
    make_event(AgentEventKind::ToolCall {
        tool_name: name.to_string(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: serde_json::json!({}),
    })
}

fn tool_call_with_input(name: &str, input: serde_json::Value) -> AgentEvent {
    make_event(AgentEventKind::ToolCall {
        tool_name: name.to_string(),
        tool_use_id: Some("tc-1".to_string()),
        parent_tool_use_id: None,
        input,
    })
}

fn tool_result(name: &str, output: serde_json::Value) -> AgentEvent {
    make_event(AgentEventKind::ToolResult {
        tool_name: name.to_string(),
        tool_use_id: None,
        output,
        is_error: false,
    })
}

fn tool_result_error(name: &str) -> AgentEvent {
    make_event(AgentEventKind::ToolResult {
        tool_name: name.to_string(),
        tool_use_id: None,
        output: serde_json::json!({"error": "failed"}),
        is_error: true,
    })
}

fn warning(msg: &str) -> AgentEvent {
    make_event(AgentEventKind::Warning {
        message: msg.to_string(),
    })
}

fn run_started() -> AgentEvent {
    make_event(AgentEventKind::RunStarted {
        message: "started".to_string(),
    })
}

fn run_completed() -> AgentEvent {
    make_event(AgentEventKind::RunCompleted {
        message: "done".to_string(),
    })
}

fn file_changed(path: &str) -> AgentEvent {
    make_event(AgentEventKind::FileChanged {
        path: path.to_string(),
        summary: "modified".to_string(),
    })
}

fn command_executed(cmd: &str, exit_code: Option<i32>) -> AgentEvent {
    make_event(AgentEventKind::CommandExecuted {
        command: cmd.to_string(),
        exit_code,
        output_preview: None,
    })
}

fn all_event_kinds() -> Vec<AgentEvent> {
    vec![
        run_started(),
        run_completed(),
        delta("text"),
        message("full"),
        tool_call("read_file"),
        tool_result("read_file", serde_json::json!("contents")),
        file_changed("src/main.rs"),
        command_executed("cargo build", Some(0)),
        warning("careful"),
        error("oops"),
    ]
}

// ===========================================================================
// 1. Empty pipeline passes events through unchanged
// ===========================================================================

#[test]
fn empty_pipeline_passes_delta_through() {
    let p = StreamPipeline::new();
    let ev = delta("hello");
    let result = p.process(ev.clone());
    assert!(result.is_some());
    assert_eq!(result.unwrap().ts, ev.ts);
}

#[test]
fn empty_pipeline_passes_every_event_kind() {
    let p = StreamPipeline::new();
    for ev in all_event_kinds() {
        assert!(
            p.process(ev).is_some(),
            "empty pipeline should pass all events"
        );
    }
}

#[test]
fn empty_pipeline_preserves_ext() {
    let p = StreamPipeline::new();
    let mut ext = BTreeMap::new();
    ext.insert("key".to_string(), serde_json::json!("value"));
    let ev = make_event_with_ext(
        AgentEventKind::AssistantDelta {
            text: "hi".to_string(),
        },
        ext.clone(),
    );
    let result = p.process(ev).unwrap();
    assert_eq!(result.ext.as_ref().unwrap().get("key"), ext.get("key"));
}

#[test]
fn default_pipeline_is_empty() {
    let p = StreamPipeline::default();
    assert!(p.recorder().is_none());
    assert!(p.stats().is_none());
    assert!(p.process(delta("x")).is_some());
}

// ===========================================================================
// 2. Filter removes matching events
// ===========================================================================

#[test]
fn filter_by_kind_keeps_matching() {
    let f = EventFilter::by_kind("assistant_delta");
    assert!(f.matches(&delta("hello")));
}

#[test]
fn filter_by_kind_rejects_non_matching() {
    let f = EventFilter::by_kind("assistant_delta");
    assert!(!f.matches(&error("nope")));
    assert!(!f.matches(&tool_call("x")));
    assert!(!f.matches(&run_started()));
}

#[test]
fn filter_errors_only_accepts_only_errors() {
    let f = EventFilter::errors_only();
    for ev in all_event_kinds() {
        let is_error = matches!(ev.kind, AgentEventKind::Error { .. });
        assert_eq!(f.matches(&ev), is_error);
    }
}

#[test]
fn filter_exclude_errors_rejects_only_errors() {
    let f = EventFilter::exclude_errors();
    for ev in all_event_kinds() {
        let is_error = matches!(ev.kind, AgentEventKind::Error { .. });
        assert_eq!(f.matches(&ev), !is_error);
    }
}

#[test]
fn filter_custom_predicate_on_text_length() {
    let f = EventFilter::new(
        |ev| matches!(&ev.kind, AgentEventKind::AssistantDelta { text } if text.len() >= 5),
    );
    assert!(f.matches(&delta("hello")));
    assert!(!f.matches(&delta("hi")));
}

#[test]
fn filter_custom_on_tool_name() {
    let f = EventFilter::new(
        |ev| matches!(&ev.kind, AgentEventKind::ToolCall { tool_name, .. } if tool_name.starts_with("read")),
    );
    assert!(f.matches(&tool_call("read_file")));
    assert!(!f.matches(&tool_call("write_file")));
}

#[test]
fn filter_by_kind_all_variant_names() {
    let kinds = [
        ("run_started", run_started()),
        ("run_completed", run_completed()),
        ("assistant_delta", delta("x")),
        ("assistant_message", message("x")),
        ("tool_call", tool_call("t")),
        ("tool_result", tool_result("t", serde_json::json!("output"))),
        ("file_changed", file_changed("f.rs")),
        ("command_executed", command_executed("ls", None)),
        ("warning", warning("w")),
        ("error", error("e")),
    ];
    for (name, ev) in &kinds {
        let f = EventFilter::by_kind(name);
        assert!(f.matches(ev), "filter by_kind({name}) should match");
    }
}

// ===========================================================================
// 3. Transform modifies events in place
// ===========================================================================

#[test]
fn transform_adds_ext_metadata() {
    let t = EventTransform::new(|mut ev| {
        let ext = ev.ext.get_or_insert_with(BTreeMap::new);
        ext.insert("source".to_string(), serde_json::json!("pipeline"));
        ev
    });
    let result = t.apply(delta("hi"));
    assert_eq!(
        result.ext.unwrap().get("source").unwrap(),
        &serde_json::json!("pipeline")
    );
}

#[test]
fn transform_replaces_delta_text() {
    let t = EventTransform::new(|mut ev| {
        if let AgentEventKind::AssistantDelta { ref mut text } = ev.kind {
            *text = text.to_uppercase();
        }
        ev
    });
    let result = t.apply(delta("hello"));
    match &result.kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "HELLO"),
        _ => panic!("expected AssistantDelta"),
    }
}

#[test]
fn transform_chain_order_matters() {
    let t1 = EventTransform::new(|mut ev| {
        if let AgentEventKind::AssistantDelta { ref mut text } = ev.kind {
            text.push_str("-first");
        }
        ev
    });
    let t2 = EventTransform::new(|mut ev| {
        if let AgentEventKind::AssistantDelta { ref mut text } = ev.kind {
            text.push_str("-second");
        }
        ev
    });
    let ev = t2.apply(t1.apply(delta("start")));
    match &ev.kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "start-first-second"),
        _ => panic!("expected AssistantDelta"),
    }
}

#[test]
fn transform_preserves_timestamp() {
    let t = EventTransform::new(|mut ev| {
        let ext = ev.ext.get_or_insert_with(BTreeMap::new);
        ext.insert("tag".to_string(), serde_json::json!(42));
        ev
    });
    let ev = delta("x");
    let ts = ev.ts;
    let result = t.apply(ev);
    assert_eq!(result.ts, ts);
}

// ===========================================================================
// 4. Recorder captures events without modifying them
// ===========================================================================

#[test]
fn recorder_starts_empty() {
    let r = EventRecorder::new();
    assert!(r.is_empty());
    assert_eq!(r.len(), 0);
    assert!(r.events().is_empty());
}

#[test]
fn recorder_captures_multiple_events() {
    let r = EventRecorder::new();
    r.record(&delta("a"));
    r.record(&error("e"));
    r.record(&tool_call("t"));
    assert_eq!(r.len(), 3);
}

#[test]
fn recorder_events_snapshot_is_independent() {
    let r = EventRecorder::new();
    r.record(&delta("a"));
    let snapshot = r.events();
    r.record(&delta("b"));
    assert_eq!(snapshot.len(), 1);
    assert_eq!(r.len(), 2);
}

#[test]
fn recorder_clear_empties() {
    let r = EventRecorder::new();
    r.record(&delta("a"));
    r.record(&delta("b"));
    r.clear();
    assert!(r.is_empty());
    assert_eq!(r.len(), 0);
}

#[test]
fn recorder_clone_shares_storage() {
    let r = EventRecorder::new();
    let r2 = r.clone();
    r.record(&delta("a"));
    assert_eq!(r2.len(), 1);
    r2.record(&delta("b"));
    assert_eq!(r.len(), 2);
}

#[test]
fn recorder_does_not_modify_original_event() {
    let r = EventRecorder::new();
    let ev = delta("original");
    let ts = ev.ts;
    r.record(&ev);
    let captured = &r.events()[0];
    assert_eq!(captured.ts, ts);
    match &captured.kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "original"),
        _ => panic!("unexpected kind"),
    }
}

// ===========================================================================
// 5. Stats accumulator counts events by kind
// ===========================================================================

#[test]
fn stats_starts_at_zero() {
    let s = EventStats::new();
    assert_eq!(s.total_events(), 0);
    assert_eq!(s.total_delta_bytes(), 0);
    assert_eq!(s.error_count(), 0);
    assert!(s.kind_counts().is_empty());
}

#[test]
fn stats_counts_each_kind_separately() {
    let s = EventStats::new();
    s.observe(&delta("a"));
    s.observe(&delta("bb"));
    s.observe(&error("e"));
    s.observe(&tool_call("t"));
    s.observe(&warning("w"));
    assert_eq!(s.count_for("assistant_delta"), 2);
    assert_eq!(s.count_for("error"), 1);
    assert_eq!(s.count_for("tool_call"), 1);
    assert_eq!(s.count_for("warning"), 1);
    assert_eq!(s.count_for("nonexistent"), 0);
    assert_eq!(s.total_events(), 5);
}

#[test]
fn stats_tracks_delta_bytes_correctly() {
    let s = EventStats::new();
    s.observe(&delta("abc")); // 3
    s.observe(&delta("defgh")); // 5
    s.observe(&delta("")); // 0
    assert_eq!(s.total_delta_bytes(), 8);
}

#[test]
fn stats_delta_bytes_ignores_non_delta() {
    let s = EventStats::new();
    s.observe(&message("this is long text"));
    s.observe(&error("err"));
    assert_eq!(s.total_delta_bytes(), 0);
}

#[test]
fn stats_error_count_tracks_only_errors() {
    let s = EventStats::new();
    s.observe(&error("e1"));
    s.observe(&warning("w"));
    s.observe(&error("e2"));
    s.observe(&tool_result_error("t"));
    assert_eq!(s.error_count(), 2); // only Error events, not tool result errors
}

#[test]
fn stats_reset_clears_everything() {
    let s = EventStats::new();
    for ev in all_event_kinds() {
        s.observe(&ev);
    }
    assert!(s.total_events() > 0);
    s.reset();
    assert_eq!(s.total_events(), 0);
    assert_eq!(s.total_delta_bytes(), 0);
    assert_eq!(s.error_count(), 0);
    assert!(s.kind_counts().is_empty());
}

#[test]
fn stats_kind_counts_snapshot() {
    let s = EventStats::new();
    s.observe(&run_started());
    s.observe(&delta("x"));
    s.observe(&run_completed());
    let counts = s.kind_counts();
    assert_eq!(counts.len(), 3);
    assert_eq!(counts["run_started"], 1);
    assert_eq!(counts["assistant_delta"], 1);
    assert_eq!(counts["run_completed"], 1);
}

#[test]
fn stats_all_event_kinds_counted() {
    let s = EventStats::new();
    let events = all_event_kinds();
    let n = events.len() as u64;
    for ev in &events {
        s.observe(ev);
    }
    assert_eq!(s.total_events(), n);
    // Each kind appears exactly once
    for (name, count) in s.kind_counts() {
        assert_eq!(count, 1, "kind {name} should appear once");
    }
}

// ===========================================================================
// 6. Pipeline composition (filter + transform + recorder)
// ===========================================================================

#[test]
fn pipeline_filter_plus_transform_plus_recorder() {
    let recorder = EventRecorder::new();
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .transform(EventTransform::new(|mut ev| {
            let ext = ev.ext.get_or_insert_with(BTreeMap::new);
            ext.insert("tagged".to_string(), serde_json::json!(true));
            ev
        }))
        .with_recorder(recorder.clone())
        .with_stats(stats.clone())
        .build();

    p.process(delta("a"));
    p.process(error("e"));
    p.process(tool_call("read"));
    p.process(warning("w"));

    assert_eq!(recorder.len(), 3); // error filtered
    assert_eq!(stats.total_events(), 3);
    assert_eq!(stats.error_count(), 0);
    for ev in recorder.events() {
        assert!(ev.ext.as_ref().unwrap().contains_key("tagged"));
    }
}

#[test]
fn pipeline_multiple_filters_are_conjunctive() {
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .filter(EventFilter::by_kind("assistant_delta"))
        .build();

    assert!(p.process(delta("ok")).is_some());
    assert!(p.process(error("bad")).is_none());
    assert!(p.process(tool_call("t")).is_none());
    assert!(p.process(warning("w")).is_none());
}

#[test]
fn pipeline_multiple_transforms_applied_sequentially() {
    let p = StreamPipelineBuilder::new()
        .transform(EventTransform::new(|mut ev| {
            if let AgentEventKind::AssistantDelta { ref mut text } = ev.kind {
                *text = text.to_uppercase();
            }
            ev
        }))
        .transform(EventTransform::new(|mut ev| {
            if let AgentEventKind::AssistantDelta { ref mut text } = ev.kind {
                text.push('!');
            }
            ev
        }))
        .build();

    let result = p.process(delta("hello")).unwrap();
    match &result.kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "HELLO!"),
        _ => panic!("expected AssistantDelta"),
    }
}

#[test]
fn pipeline_recorder_sees_transformed_events() {
    let recorder = EventRecorder::new();
    let p = StreamPipelineBuilder::new()
        .transform(EventTransform::new(|mut ev| {
            if let AgentEventKind::AssistantDelta { ref mut text } = ev.kind {
                *text = format!("[modified] {text}");
            }
            ev
        }))
        .with_recorder(recorder.clone())
        .build();

    p.process(delta("original"));
    let recorded = &recorder.events()[0];
    match &recorded.kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "[modified] original"),
        _ => panic!("expected AssistantDelta"),
    }
}

#[test]
fn pipeline_stats_see_post_transform_events() {
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .transform(EventTransform::new(|mut ev| {
            if let AgentEventKind::AssistantDelta { ref mut text } = ev.kind {
                *text = format!("{text}+extra");
            }
            ev
        }))
        .with_stats(stats.clone())
        .build();

    p.process(delta("hi")); // "hi+extra" = 8 bytes
    assert_eq!(stats.total_delta_bytes(), 8);
}

// ===========================================================================
// 7. Pipeline ordering matters (filter before transform vs after)
// ===========================================================================

#[test]
fn filter_before_transform_filters_original() {
    // Filter only allows deltas with text > 3 chars, then transform uppercases
    let recorder = EventRecorder::new();
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::new(
            |ev| matches!(&ev.kind, AgentEventKind::AssistantDelta { text } if text.len() > 3),
        ))
        .transform(EventTransform::new(|mut ev| {
            if let AgentEventKind::AssistantDelta { ref mut text } = ev.kind {
                *text = text.to_uppercase();
            }
            ev
        }))
        .with_recorder(recorder.clone())
        .build();

    p.process(delta("hi")); // filtered out (len=2)
    p.process(delta("hello")); // passes, uppercased
    assert_eq!(recorder.len(), 1);
    match &recorder.events()[0].kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "HELLO"),
        _ => panic!("expected AssistantDelta"),
    }
}

#[test]
fn pipeline_filters_run_before_transforms_always() {
    // Even if we think "transform first", the pipeline processes filters first.
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::errors_only())
        .transform(EventTransform::new(|mut ev| {
            let ext = ev.ext.get_or_insert_with(BTreeMap::new);
            ext.insert("reached_transform".to_string(), serde_json::json!(true));
            ev
        }))
        .with_stats(stats.clone())
        .build();

    // Delta should be filtered before reaching transform or stats
    assert!(p.process(delta("x")).is_none());
    assert_eq!(stats.total_events(), 0);

    // Error passes filter and reaches transform/stats
    let result = p.process(error("e")).unwrap();
    assert!(
        result
            .ext
            .as_ref()
            .unwrap()
            .contains_key("reached_transform")
    );
    assert_eq!(stats.total_events(), 1);
}

// ===========================================================================
// 8. Identity transform is a no-op
// ===========================================================================

#[test]
fn identity_transform_preserves_delta() {
    let t = EventTransform::identity();
    let ev = delta("test");
    let ts = ev.ts;
    let result = t.apply(ev);
    assert_eq!(result.ts, ts);
    match &result.kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "test"),
        _ => panic!("expected AssistantDelta"),
    }
}

#[test]
fn identity_transform_preserves_all_event_kinds() {
    let t = EventTransform::identity();
    for ev in all_event_kinds() {
        let kind_before = event_kind_name(&ev.kind);
        let result = t.apply(ev);
        assert_eq!(event_kind_name(&result.kind), kind_before);
    }
}

#[test]
fn identity_transform_preserves_ext() {
    let t = EventTransform::identity();
    let mut ext = BTreeMap::new();
    ext.insert("k".to_string(), serde_json::json!("v"));
    let ev = make_event_with_ext(
        AgentEventKind::AssistantDelta {
            text: "x".to_string(),
        },
        ext,
    );
    let result = t.apply(ev);
    assert_eq!(
        result.ext.as_ref().unwrap().get("k").unwrap(),
        &serde_json::json!("v")
    );
}

#[test]
fn pipeline_with_only_identity_transform() {
    let recorder = EventRecorder::new();
    let p = StreamPipelineBuilder::new()
        .transform(EventTransform::identity())
        .with_recorder(recorder.clone())
        .build();
    for ev in all_event_kinds() {
        p.process(ev);
    }
    assert_eq!(recorder.len(), 10);
}

// ===========================================================================
// 9. Filter-all removes everything
// ===========================================================================

#[test]
fn filter_all_rejects_everything() {
    let f = EventFilter::new(|_| false);
    for ev in all_event_kinds() {
        assert!(!f.matches(&ev));
    }
}

#[test]
fn pipeline_with_reject_all_filter() {
    let recorder = EventRecorder::new();
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::new(|_| false))
        .with_recorder(recorder.clone())
        .with_stats(stats.clone())
        .build();

    for ev in all_event_kinds() {
        assert!(p.process(ev).is_none());
    }
    assert_eq!(recorder.len(), 0);
    assert_eq!(stats.total_events(), 0);
}

#[test]
fn filter_accept_all_passes_everything() {
    let f = EventFilter::new(|_| true);
    for ev in all_event_kinds() {
        assert!(f.matches(&ev));
    }
}

#[test]
fn pipeline_with_accept_all_filter() {
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::new(|_| true))
        .build();
    for ev in all_event_kinds() {
        assert!(p.process(ev).is_some());
    }
}

// ===========================================================================
// 10. Large event streams (1000+ events)
// ===========================================================================

#[test]
fn pipeline_processes_1000_events() {
    let stats = EventStats::new();
    let recorder = EventRecorder::new();
    let p = StreamPipelineBuilder::new()
        .with_stats(stats.clone())
        .with_recorder(recorder.clone())
        .build();

    for i in 0..1000 {
        p.process(delta(&format!("token-{i}")));
    }
    assert_eq!(stats.total_events(), 1000);
    assert_eq!(recorder.len(), 1000);
}

#[test]
fn pipeline_filters_half_of_2000_events() {
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .with_stats(stats.clone())
        .build();

    for i in 0..2000 {
        if i % 2 == 0 {
            p.process(delta(&format!("ok-{i}")));
        } else {
            p.process(error(&format!("err-{i}")));
        }
    }
    assert_eq!(stats.total_events(), 1000);
}

#[tokio::test]
async fn stream_pipe_1000_events_through_pipeline() {
    let (tx_in, rx_in) = mpsc::channel(64);
    let (tx_out, mut rx_out) = mpsc::channel(64);

    let stats = EventStats::new();
    let pipeline = StreamPipelineBuilder::new()
        .with_stats(stats.clone())
        .build();

    let sender = tokio::spawn(async move {
        for i in 0..1000 {
            tx_in.send(delta(&format!("t-{i}"))).await.unwrap();
        }
    });

    let stream = EventStream::new(rx_in);
    let consumer = tokio::spawn(async move {
        let mut count = 0;
        while rx_out.recv().await.is_some() {
            count += 1;
        }
        count
    });

    sender.await.unwrap();
    // pipe blocks until tx_in is dropped (sender task completed drops tx_in)
    stream.pipe(&pipeline, tx_out).await;

    let count = consumer.await.unwrap();
    assert_eq!(count, 1000);
    assert_eq!(stats.total_events(), 1000);
}

#[test]
fn stats_delta_bytes_for_large_stream() {
    let s = EventStats::new();
    let text = "x".repeat(100);
    for _ in 0..500 {
        s.observe(&delta(&text));
    }
    assert_eq!(s.total_delta_bytes(), 50_000);
}

// ===========================================================================
// 11. Event kinds: all variant coverage
// ===========================================================================

#[test]
fn event_kind_name_run_started() {
    assert_eq!(
        event_kind_name(&AgentEventKind::RunStarted {
            message: String::new()
        }),
        "run_started"
    );
}

#[test]
fn event_kind_name_run_completed() {
    assert_eq!(
        event_kind_name(&AgentEventKind::RunCompleted {
            message: String::new()
        }),
        "run_completed"
    );
}

#[test]
fn event_kind_name_assistant_delta() {
    assert_eq!(
        event_kind_name(&AgentEventKind::AssistantDelta {
            text: String::new()
        }),
        "assistant_delta"
    );
}

#[test]
fn event_kind_name_assistant_message() {
    assert_eq!(
        event_kind_name(&AgentEventKind::AssistantMessage {
            text: String::new()
        }),
        "assistant_message"
    );
}

#[test]
fn event_kind_name_tool_call_variant() {
    assert_eq!(
        event_kind_name(&AgentEventKind::ToolCall {
            tool_name: String::new(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: serde_json::json!(null),
        }),
        "tool_call"
    );
}

#[test]
fn event_kind_name_tool_result_variant() {
    assert_eq!(
        event_kind_name(&AgentEventKind::ToolResult {
            tool_name: String::new(),
            tool_use_id: None,
            output: serde_json::json!(null),
            is_error: false,
        }),
        "tool_result"
    );
}

#[test]
fn event_kind_name_file_changed_variant() {
    assert_eq!(
        event_kind_name(&AgentEventKind::FileChanged {
            path: String::new(),
            summary: String::new()
        }),
        "file_changed"
    );
}

#[test]
fn event_kind_name_command_executed_variant() {
    assert_eq!(
        event_kind_name(&AgentEventKind::CommandExecuted {
            command: String::new(),
            exit_code: None,
            output_preview: None,
        }),
        "command_executed"
    );
}

#[test]
fn event_kind_name_warning_variant() {
    assert_eq!(
        event_kind_name(&AgentEventKind::Warning {
            message: String::new()
        }),
        "warning"
    );
}

#[test]
fn event_kind_name_error_variant() {
    assert_eq!(
        event_kind_name(&AgentEventKind::Error {
            message: String::new(),
            error_code: None,
        }),
        "error"
    );
}

#[test]
fn pipeline_processes_tool_result_with_error_flag() {
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .with_stats(stats.clone())
        .build();
    p.process(tool_result_error("failing_tool"));
    assert_eq!(stats.count_for("tool_result"), 1);
    // ToolResult with is_error=true is not AgentEventKind::Error
    assert_eq!(stats.error_count(), 0);
}

#[test]
fn pipeline_processes_command_with_exit_code() {
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .with_stats(stats.clone())
        .build();
    p.process(command_executed("cargo test", Some(1)));
    p.process(command_executed("cargo build", Some(0)));
    assert_eq!(stats.count_for("command_executed"), 2);
}

// ===========================================================================
// 12. Pipeline serde roundtrip (AgentEvent serialization)
// ===========================================================================

#[test]
fn agent_event_serde_roundtrip_delta() {
    let ev = delta("hello world");
    let json = serde_json::to_string(&ev).unwrap();
    let deserialized: AgentEvent = serde_json::from_str(&json).unwrap();
    match &deserialized.kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "hello world"),
        _ => panic!("unexpected kind after roundtrip"),
    }
}

#[test]
fn agent_event_serde_roundtrip_error_with_code() {
    let ev = error_with_code("bad request", abp_error::ErrorCode::BackendTimeout);
    let json = serde_json::to_string(&ev).unwrap();
    let deserialized: AgentEvent = serde_json::from_str(&json).unwrap();
    match &deserialized.kind {
        AgentEventKind::Error {
            message,
            error_code,
        } => {
            assert_eq!(message, "bad request");
            assert_eq!(*error_code, Some(abp_error::ErrorCode::BackendTimeout));
        }
        _ => panic!("unexpected kind"),
    }
}

#[test]
fn agent_event_serde_roundtrip_tool_call_with_input() {
    let ev = tool_call_with_input("search", serde_json::json!({"query": "rust async"}));
    let json = serde_json::to_string(&ev).unwrap();
    let deserialized: AgentEvent = serde_json::from_str(&json).unwrap();
    match &deserialized.kind {
        AgentEventKind::ToolCall {
            tool_name, input, ..
        } => {
            assert_eq!(tool_name, "search");
            assert_eq!(input["query"], "rust async");
        }
        _ => panic!("unexpected kind"),
    }
}

#[test]
fn agent_event_serde_roundtrip_with_ext() {
    let mut ext = BTreeMap::new();
    ext.insert("custom_key".to_string(), serde_json::json!(42));
    let ev = make_event_with_ext(
        AgentEventKind::AssistantDelta {
            text: "x".to_string(),
        },
        ext,
    );
    let json = serde_json::to_string(&ev).unwrap();
    let deserialized: AgentEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(
        deserialized
            .ext
            .as_ref()
            .unwrap()
            .get("custom_key")
            .unwrap(),
        &serde_json::json!(42)
    );
}

#[test]
fn pipeline_output_is_serializable() {
    let p = StreamPipelineBuilder::new()
        .transform(EventTransform::new(|mut ev| {
            let ext = ev.ext.get_or_insert_with(BTreeMap::new);
            ext.insert("processed".to_string(), serde_json::json!(true));
            ev
        }))
        .build();

    let result = p.process(delta("data")).unwrap();
    let json = serde_json::to_string(&result).unwrap();
    let deserialized: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(deserialized.ext.as_ref().unwrap().contains_key("processed"));
}

// ===========================================================================
// 13. Concurrent pipeline execution
// ===========================================================================

#[tokio::test]
async fn concurrent_recording_from_multiple_tasks() {
    let recorder = EventRecorder::new();
    let stats = EventStats::new();
    let pipeline = StreamPipelineBuilder::new()
        .with_recorder(recorder.clone())
        .with_stats(stats.clone())
        .build();

    let mut handles = Vec::new();
    for task_id in 0..10 {
        let p = pipeline.clone();
        handles.push(tokio::spawn(async move {
            for i in 0..100 {
                p.process(delta(&format!("task{task_id}-{i}")));
            }
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    assert_eq!(recorder.len(), 1000);
    assert_eq!(stats.total_events(), 1000);
}

#[tokio::test]
async fn concurrent_stats_observation() {
    let stats = EventStats::new();
    let mut handles = Vec::new();
    for _ in 0..5 {
        let s = stats.clone();
        handles.push(tokio::spawn(async move {
            for _ in 0..200 {
                s.observe(&delta("x"));
            }
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    assert_eq!(stats.total_events(), 1000);
    assert_eq!(stats.total_delta_bytes(), 1000); // "x" is 1 byte each
}

#[tokio::test]
async fn multiplexer_three_streams() {
    let ts_base = Utc::now();
    let mut receivers = Vec::new();

    for i in 0..3u32 {
        let (tx, rx) = mpsc::channel(16);
        let ts = ts_base + chrono::Duration::milliseconds(i as i64 * 10);
        tx.send(AgentEvent {
            ts,
            kind: AgentEventKind::AssistantDelta {
                text: format!("stream-{i}"),
            },
            ext: None,
        })
        .await
        .unwrap();
        drop(tx);
        receivers.push(rx);
    }

    let mux = EventMultiplexer::new(receivers);
    let events = mux.collect_sorted().await;
    assert_eq!(events.len(), 3);
    // Should be sorted by timestamp
    assert!(events[0].ts <= events[1].ts);
    assert!(events[1].ts <= events[2].ts);
}

#[tokio::test]
async fn stream_pipe_concurrent_producer_consumer() {
    let (tx_in, rx_in) = mpsc::channel(32);
    let (tx_out, mut rx_out) = mpsc::channel(32);

    let recorder = EventRecorder::new();
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .with_recorder(recorder.clone())
        .build();

    // Producer: mixed events
    let producer = tokio::spawn(async move {
        for i in 0..50 {
            if i % 5 == 0 {
                tx_in.send(error(&format!("err-{i}"))).await.unwrap();
            } else {
                tx_in.send(delta(&format!("ok-{i}"))).await.unwrap();
            }
        }
    });

    // Consumer
    let consumer = tokio::spawn(async move {
        let mut count = 0;
        while rx_out.recv().await.is_some() {
            count += 1;
        }
        count
    });

    producer.await.unwrap();
    let stream = EventStream::new(rx_in);
    stream.pipe(&pipeline, tx_out).await;

    let count = consumer.await.unwrap();
    assert_eq!(count, 40); // 50 total - 10 errors
    assert_eq!(recorder.len(), 40);
}

// ===========================================================================
// 14. Pipeline builder pattern
// ===========================================================================

#[test]
fn builder_creates_empty_pipeline() {
    let p = StreamPipelineBuilder::new().build();
    assert!(p.recorder().is_none());
    assert!(p.stats().is_none());
    assert!(p.process(delta("x")).is_some());
}

#[test]
fn builder_record_enables_recorder() {
    let p = StreamPipelineBuilder::new().record().build();
    assert!(p.recorder().is_some());
    p.process(delta("a"));
    assert_eq!(p.recorder().unwrap().len(), 1);
}

#[test]
fn builder_with_external_recorder() {
    let external = EventRecorder::new();
    let p = StreamPipelineBuilder::new()
        .with_recorder(external.clone())
        .build();
    p.process(delta("a"));
    assert_eq!(external.len(), 1);
    assert_eq!(p.recorder().unwrap().len(), 1);
}

#[test]
fn builder_with_stats_enables_stats() {
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .with_stats(stats.clone())
        .build();
    p.process(delta("a"));
    assert_eq!(stats.total_events(), 1);
    assert_eq!(p.stats().unwrap().total_events(), 1);
}

#[test]
fn builder_chaining_multiple_filters() {
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .filter(EventFilter::new(|ev| {
            !matches!(ev.kind, AgentEventKind::Warning { .. })
        }))
        .build();

    assert!(p.process(delta("ok")).is_some());
    assert!(p.process(error("e")).is_none());
    assert!(p.process(warning("w")).is_none());
    assert!(p.process(tool_call("t")).is_some());
}

#[test]
fn builder_chaining_multiple_transforms() {
    let p = StreamPipelineBuilder::new()
        .transform(EventTransform::new(|mut ev| {
            let ext = ev.ext.get_or_insert_with(BTreeMap::new);
            ext.insert("step1".to_string(), serde_json::json!(true));
            ev
        }))
        .transform(EventTransform::new(|mut ev| {
            let ext = ev.ext.get_or_insert_with(BTreeMap::new);
            ext.insert("step2".to_string(), serde_json::json!(true));
            ev
        }))
        .build();

    let result = p.process(delta("x")).unwrap();
    let ext = result.ext.unwrap();
    assert!(ext.contains_key("step1"));
    assert!(ext.contains_key("step2"));
}

#[test]
fn builder_full_composition() {
    let recorder = EventRecorder::new();
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .filter(EventFilter::new(|ev| {
            !matches!(ev.kind, AgentEventKind::Warning { .. })
        }))
        .transform(EventTransform::new(|mut ev| {
            let ext = ev.ext.get_or_insert_with(BTreeMap::new);
            ext.insert("processed".to_string(), serde_json::json!(true));
            ev
        }))
        .transform(EventTransform::identity())
        .with_recorder(recorder.clone())
        .with_stats(stats.clone())
        .build();

    p.process(delta("a"));
    p.process(error("e"));
    p.process(warning("w"));
    p.process(tool_call("t"));

    assert_eq!(recorder.len(), 2); // delta + tool_call
    assert_eq!(stats.total_events(), 2);
}

// ===========================================================================
// 15. Edge cases: null content, empty strings, very large payloads
// ===========================================================================

#[test]
fn empty_string_delta_passes_through() {
    let p = StreamPipeline::new();
    let result = p.process(delta("")).unwrap();
    match &result.kind {
        AgentEventKind::AssistantDelta { text } => assert!(text.is_empty()),
        _ => panic!("expected AssistantDelta"),
    }
}

#[test]
fn empty_string_error_message() {
    let p = StreamPipeline::new();
    let ev = make_event(AgentEventKind::Error {
        message: String::new(),
        error_code: None,
    });
    assert!(p.process(ev).is_some());
}

#[test]
fn very_large_delta_payload() {
    let large_text = "x".repeat(1_000_000); // 1MB
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .with_stats(stats.clone())
        .build();
    p.process(delta(&large_text));
    assert_eq!(stats.total_delta_bytes(), 1_000_000);
    assert_eq!(stats.total_events(), 1);
}

#[test]
fn tool_call_with_null_input() {
    let p = StreamPipeline::new();
    let ev = make_event(AgentEventKind::ToolCall {
        tool_name: "test".to_string(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: serde_json::Value::Null,
    });
    assert!(p.process(ev).is_some());
}

#[test]
fn tool_result_with_null_output() {
    let p = StreamPipeline::new();
    let ev = make_event(AgentEventKind::ToolResult {
        tool_name: "test".to_string(),
        tool_use_id: None,
        output: serde_json::Value::Null,
        is_error: false,
    });
    assert!(p.process(ev).is_some());
}

#[test]
fn event_with_none_ext() {
    let p = StreamPipeline::new();
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta {
            text: "x".to_string(),
        },
        ext: None,
    };
    let result = p.process(ev).unwrap();
    assert!(result.ext.is_none());
}

#[test]
fn event_with_empty_ext_map() {
    let p = StreamPipeline::new();
    let ev = make_event_with_ext(
        AgentEventKind::AssistantDelta {
            text: "x".to_string(),
        },
        BTreeMap::new(),
    );
    let result = p.process(ev).unwrap();
    assert!(result.ext.unwrap().is_empty());
}

#[test]
fn tool_call_with_large_json_input() {
    let mut big_obj = serde_json::Map::new();
    for i in 0..1000 {
        big_obj.insert(format!("key_{i}"), serde_json::json!(i));
    }
    let ev = make_event(AgentEventKind::ToolCall {
        tool_name: "big_tool".to_string(),
        tool_use_id: Some("tc-big".to_string()),
        parent_tool_use_id: None,
        input: serde_json::Value::Object(big_obj),
    });
    let p = StreamPipeline::new();
    assert!(p.process(ev).is_some());
}

#[test]
fn unicode_in_delta_text() {
    let p = StreamPipeline::new();
    let result = p.process(delta("こんにちは 🌍 émojis")).unwrap();
    match &result.kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "こんにちは 🌍 émojis"),
        _ => panic!("expected AssistantDelta"),
    }
}

#[test]
fn stats_unicode_delta_bytes() {
    let s = EventStats::new();
    // UTF-8 byte length, not char count
    let text = "こんにちは"; // 5 chars × 3 bytes = 15 bytes
    s.observe(&delta(text));
    assert_eq!(s.total_delta_bytes(), text.len() as u64);
}

#[test]
fn filter_on_ext_field() {
    let f = EventFilter::new(|ev| {
        ev.ext
            .as_ref()
            .and_then(|e| e.get("priority"))
            .and_then(|v| v.as_str())
            == Some("high")
    });

    let mut ext = BTreeMap::new();
    ext.insert("priority".to_string(), serde_json::json!("high"));
    let ev_high = make_event_with_ext(
        AgentEventKind::AssistantDelta {
            text: "x".to_string(),
        },
        ext,
    );
    assert!(f.matches(&ev_high));
    assert!(!f.matches(&delta("no ext")));
}

#[test]
fn command_executed_with_output_preview() {
    let p = StreamPipeline::new();
    let ev = make_event(AgentEventKind::CommandExecuted {
        command: "echo hello".to_string(),
        exit_code: Some(0),
        output_preview: Some("hello\n".to_string()),
    });
    assert!(p.process(ev).is_some());
}

#[test]
fn file_changed_with_empty_path() {
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .with_stats(stats.clone())
        .build();
    p.process(file_changed(""));
    assert_eq!(stats.count_for("file_changed"), 1);
}

// ===========================================================================
// Additional edge cases and integration
// ===========================================================================

#[test]
fn pipeline_process_returns_none_for_filtered_event() {
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::errors_only())
        .build();
    assert!(p.process(delta("x")).is_none());
}

#[test]
fn pipeline_process_returns_some_for_passing_event() {
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::errors_only())
        .build();
    assert!(p.process(error("e")).is_some());
}

#[tokio::test]
async fn event_stream_recv_returns_events_in_order() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("first")).await.unwrap();
    tx.send(delta("second")).await.unwrap();
    tx.send(delta("third")).await.unwrap();
    drop(tx);

    let mut stream = EventStream::new(rx);
    let ev1 = stream.recv().await.unwrap();
    let ev2 = stream.recv().await.unwrap();
    let ev3 = stream.recv().await.unwrap();
    assert!(stream.recv().await.is_none());

    match (&ev1.kind, &ev2.kind, &ev3.kind) {
        (
            AgentEventKind::AssistantDelta { text: t1 },
            AgentEventKind::AssistantDelta { text: t2 },
            AgentEventKind::AssistantDelta { text: t3 },
        ) => {
            assert_eq!(t1, "first");
            assert_eq!(t2, "second");
            assert_eq!(t3, "third");
        }
        _ => panic!("unexpected kinds"),
    }
}

#[tokio::test]
async fn event_stream_into_inner() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("x")).await.unwrap();
    drop(tx);

    let stream = EventStream::new(rx);
    let mut inner = stream.into_inner();
    let ev = inner.recv().await.unwrap();
    match &ev.kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "x"),
        _ => panic!("unexpected kind"),
    }
}

#[test]
fn recorder_with_all_event_kinds() {
    let r = EventRecorder::new();
    let events = all_event_kinds();
    for ev in &events {
        r.record(ev);
    }
    assert_eq!(r.len(), events.len());
}

#[test]
fn stats_clone_shares_state() {
    let s = EventStats::new();
    let s2 = s.clone();
    s.observe(&delta("a"));
    assert_eq!(s2.total_events(), 1);
    s2.observe(&error("e"));
    assert_eq!(s.error_count(), 1);
}

#[test]
fn pipeline_clone_shares_recorder_and_stats() {
    let recorder = EventRecorder::new();
    let stats = EventStats::new();
    let p1 = StreamPipelineBuilder::new()
        .with_recorder(recorder.clone())
        .with_stats(stats.clone())
        .build();
    let p2 = p1.clone();

    p1.process(delta("a"));
    p2.process(delta("b"));

    assert_eq!(recorder.len(), 2);
    assert_eq!(stats.total_events(), 2);
}

#[test]
fn filter_debug_impl() {
    let f = EventFilter::by_kind("error");
    let debug = format!("{f:?}");
    assert!(debug.contains("EventFilter"));
}

#[test]
fn transform_debug_impl() {
    let t = EventTransform::identity();
    let debug = format!("{t:?}");
    assert!(debug.contains("EventTransform"));
}

#[test]
fn recorder_debug_impl() {
    let r = EventRecorder::new();
    let debug = format!("{r:?}");
    assert!(debug.contains("EventRecorder"));
}

#[test]
fn stats_debug_impl() {
    let s = EventStats::new();
    let debug = format!("{s:?}");
    assert!(debug.contains("EventStats"));
}

#[test]
fn pipeline_debug_impl() {
    let p = StreamPipeline::new();
    let debug = format!("{p:?}");
    assert!(debug.contains("StreamPipeline"));
}

#[test]
fn builder_debug_impl() {
    let b = StreamPipelineBuilder::new();
    let debug = format!("{b:?}");
    assert!(debug.contains("StreamPipelineBuilder"));
}
