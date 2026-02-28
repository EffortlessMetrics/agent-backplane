// SPDX-License-Identifier: MIT OR Apache-2.0
use abp_core::{AgentEvent, AgentEventKind};
use chrono::{DateTime, TimeZone, Utc};
use sidecar_kit::transform::{
    EnrichTransformer, EventTransformer, FilterTransformer, RedactTransformer,
    ThrottleTransformer, TimestampTransformer, TransformerChain,
};
use std::collections::BTreeMap;

// ── helpers ──────────────────────────────────────────────────────────

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn make_event_at(kind: AgentEventKind, ts: DateTime<Utc>) -> AgentEvent {
    AgentEvent { ts, kind, ext: None }
}

fn run_started(msg: &str) -> AgentEvent {
    make_event(AgentEventKind::RunStarted {
        message: msg.to_string(),
    })
}

fn assistant_message(text: &str) -> AgentEvent {
    make_event(AgentEventKind::AssistantMessage {
        text: text.to_string(),
    })
}

fn error_event(msg: &str) -> AgentEvent {
    make_event(AgentEventKind::Error {
        message: msg.to_string(),
    })
}

// ── EventTransformer trait basics ────────────────────────────────────

#[test]
fn redact_transformer_has_correct_name() {
    let t = RedactTransformer::new(vec![]);
    assert_eq!(t.name(), "redact");
}

#[test]
fn throttle_transformer_has_correct_name() {
    let t = ThrottleTransformer::new(5);
    assert_eq!(t.name(), "throttle");
}

#[test]
fn enrich_transformer_has_correct_name() {
    let t = EnrichTransformer::new(BTreeMap::new());
    assert_eq!(t.name(), "enrich");
}

#[test]
fn filter_transformer_has_correct_name() {
    let t = FilterTransformer::new(Box::new(|_| true));
    assert_eq!(t.name(), "filter");
}

#[test]
fn timestamp_transformer_has_correct_name() {
    let t = TimestampTransformer::new();
    assert_eq!(t.name(), "timestamp");
}

// ── RedactTransformer ────────────────────────────────────────────────

#[test]
fn redact_replaces_pattern_in_message() {
    let t = RedactTransformer::new(vec!["sk-secret123".to_string()]);
    let event = run_started("Key is sk-secret123 here");
    let result = t.transform(event).unwrap();
    match &result.kind {
        AgentEventKind::RunStarted { message } => {
            assert_eq!(message, "Key is [REDACTED] here");
        }
        _ => panic!("expected RunStarted"),
    }
}

#[test]
fn redact_replaces_multiple_patterns() {
    let t = RedactTransformer::new(vec!["password".to_string(), "token".to_string()]);
    let event = assistant_message("my password and token are secret");
    let result = t.transform(event).unwrap();
    match &result.kind {
        AgentEventKind::AssistantMessage { text } => {
            assert_eq!(text, "my [REDACTED] and [REDACTED] are secret");
        }
        _ => panic!("expected AssistantMessage"),
    }
}

#[test]
fn redact_handles_tool_call_input() {
    let t = RedactTransformer::new(vec!["secret_key".to_string()]);
    let event = make_event(AgentEventKind::ToolCall {
        tool_name: "fetch".to_string(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: serde_json::json!({"api_key": "secret_key", "nested": {"val": "has secret_key"}}),
    });
    let result = t.transform(event).unwrap();
    match &result.kind {
        AgentEventKind::ToolCall { input, .. } => {
            assert_eq!(input["api_key"], "[REDACTED]");
            assert_eq!(input["nested"]["val"], "has [REDACTED]");
        }
        _ => panic!("expected ToolCall"),
    }
}

#[test]
fn redact_with_empty_patterns_is_passthrough() {
    let t = RedactTransformer::new(vec![]);
    let event = run_started("no change");
    let result = t.transform(event).unwrap();
    match &result.kind {
        AgentEventKind::RunStarted { message } => {
            assert_eq!(message, "no change");
        }
        _ => panic!("expected RunStarted"),
    }
}

#[test]
fn redact_handles_command_executed() {
    let t = RedactTransformer::new(vec!["PASSWORD=hunter2".to_string()]);
    let event = make_event(AgentEventKind::CommandExecuted {
        command: "env PASSWORD=hunter2 ./run.sh".to_string(),
        exit_code: Some(0),
        output_preview: Some("PASSWORD=hunter2 was set".to_string()),
    });
    let result = t.transform(event).unwrap();
    match &result.kind {
        AgentEventKind::CommandExecuted { command, output_preview, .. } => {
            assert_eq!(command, "env [REDACTED] ./run.sh");
            assert_eq!(output_preview.as_deref(), Some("[REDACTED] was set"));
        }
        _ => panic!("expected CommandExecuted"),
    }
}

// ── ThrottleTransformer ──────────────────────────────────────────────

#[test]
fn throttle_allows_events_within_limit() {
    let t = ThrottleTransformer::new(2);
    assert!(t.transform(run_started("1")).is_some());
    assert!(t.transform(run_started("2")).is_some());
}

#[test]
fn throttle_drops_events_exceeding_limit() {
    let t = ThrottleTransformer::new(2);
    t.transform(run_started("1"));
    t.transform(run_started("2"));
    assert!(t.transform(run_started("3")).is_none());
}

#[test]
fn throttle_tracks_kinds_independently() {
    let t = ThrottleTransformer::new(1);
    assert!(t.transform(run_started("a")).is_some());
    assert!(t.transform(error_event("b")).is_some());
    // Second of each kind should be dropped
    assert!(t.transform(run_started("c")).is_none());
    assert!(t.transform(error_event("d")).is_none());
}

// ── EnrichTransformer ────────────────────────────────────────────────

#[test]
fn enrich_adds_metadata_to_ext() {
    let mut meta = BTreeMap::new();
    meta.insert("env".to_string(), "prod".to_string());
    meta.insert("version".to_string(), "1.0".to_string());
    let t = EnrichTransformer::new(meta);
    let event = run_started("hello");
    let result = t.transform(event).unwrap();
    let ext = result.ext.unwrap();
    assert_eq!(ext["env"], serde_json::Value::String("prod".to_string()));
    assert_eq!(ext["version"], serde_json::Value::String("1.0".to_string()));
}

#[test]
fn enrich_merges_with_existing_ext() {
    let mut meta = BTreeMap::new();
    meta.insert("new_key".to_string(), "new_val".to_string());
    let t = EnrichTransformer::new(meta);

    let mut existing_ext = BTreeMap::new();
    existing_ext.insert("old_key".to_string(), serde_json::json!("old_val"));
    let mut event = run_started("hello");
    event.ext = Some(existing_ext);

    let result = t.transform(event).unwrap();
    let ext = result.ext.unwrap();
    assert_eq!(ext["old_key"], serde_json::json!("old_val"));
    assert_eq!(ext["new_key"], serde_json::json!("new_val"));
}

// ── FilterTransformer ────────────────────────────────────────────────

#[test]
fn filter_passes_matching_events() {
    let t = FilterTransformer::new(Box::new(|e| {
        matches!(&e.kind, AgentEventKind::RunStarted { .. })
    }));
    assert!(t.transform(run_started("yes")).is_some());
    assert!(t.transform(error_event("no")).is_none());
}

#[test]
fn filter_reject_all() {
    let t = FilterTransformer::new(Box::new(|_| false));
    assert!(t.transform(run_started("a")).is_none());
    assert!(t.transform(error_event("b")).is_none());
}

// ── TimestampTransformer ─────────────────────────────────────────────

#[test]
fn timestamp_replaces_epoch_timestamp() {
    let t = TimestampTransformer::new();
    let epoch = Utc.timestamp_opt(0, 0).unwrap();
    let event = make_event_at(AgentEventKind::RunStarted { message: "test".into() }, epoch);
    assert_eq!(event.ts.timestamp(), 0);
    let result = t.transform(event).unwrap();
    assert!(result.ts.timestamp() > 0);
}

#[test]
fn timestamp_preserves_valid_timestamp() {
    let t = TimestampTransformer::new();
    let now = Utc::now();
    let event = make_event_at(AgentEventKind::RunStarted { message: "test".into() }, now);
    let result = t.transform(event).unwrap();
    assert_eq!(result.ts, now);
}

// ── TransformerChain ─────────────────────────────────────────────────

#[test]
fn empty_chain_is_passthrough() {
    let chain = TransformerChain::new();
    let event = run_started("hello");
    let result = chain.process(event.clone()).unwrap();
    assert_eq!(result.ts, event.ts);
    match &result.kind {
        AgentEventKind::RunStarted { message } => assert_eq!(message, "hello"),
        _ => panic!("expected RunStarted"),
    }
}

#[test]
fn chain_applies_transformers_in_order() {
    let chain = TransformerChain::new()
        .with(Box::new(RedactTransformer::new(vec!["secret".to_string()])))
        .with(Box::new(EnrichTransformer::new({
            let mut m = BTreeMap::new();
            m.insert("stage".to_string(), "processed".to_string());
            m
        })));

    let event = run_started("my secret data");
    let result = chain.process(event).unwrap();
    match &result.kind {
        AgentEventKind::RunStarted { message } => {
            assert_eq!(message, "my [REDACTED] data");
        }
        _ => panic!("expected RunStarted"),
    }
    assert!(result.ext.is_some());
}

#[test]
fn chain_short_circuits_on_filter() {
    let chain = TransformerChain::new()
        .with(Box::new(FilterTransformer::new(Box::new(|_| false))))
        .with(Box::new(EnrichTransformer::new(BTreeMap::new())));

    assert!(chain.process(run_started("hello")).is_none());
}

#[test]
fn process_batch_filters_and_transforms() {
    let chain = TransformerChain::new()
        .with(Box::new(FilterTransformer::new(Box::new(|e| {
            matches!(&e.kind, AgentEventKind::RunStarted { .. })
        }))))
        .with(Box::new(RedactTransformer::new(vec!["key".to_string()])));

    let events = vec![
        run_started("my key"),
        error_event("fail"),
        run_started("no key here"),
        assistant_message("hello key"),
    ];

    let results = chain.process_batch(events);
    assert_eq!(results.len(), 2);
    match &results[0].kind {
        AgentEventKind::RunStarted { message } => assert_eq!(message, "my [REDACTED]"),
        _ => panic!("expected RunStarted"),
    }
    match &results[1].kind {
        AgentEventKind::RunStarted { message } => assert_eq!(message, "no [REDACTED] here"),
        _ => panic!("expected RunStarted"),
    }
}

#[test]
fn process_batch_empty_input() {
    let chain = TransformerChain::new();
    let results = chain.process_batch(vec![]);
    assert!(results.is_empty());
}

#[test]
fn chain_default_is_passthrough() {
    let chain = TransformerChain::default();
    let event = run_started("test");
    assert!(chain.process(event).is_some());
}
