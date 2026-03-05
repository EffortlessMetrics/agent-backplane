#![allow(clippy::all)]
#![allow(clippy::manual_repeat_n)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::single_component_path_imports)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unnecessary_to_owned)]
#![allow(clippy::implicit_clone)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::iter_kv_map)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::single_match)]
#![allow(clippy::manual_map)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::len_zero)]
#![allow(clippy::map_entry)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(unknown_lints)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for [`abp_core::filter::EventFilter`].

use abp_core::filter::EventFilter;
use abp_core::{AgentEvent, AgentEventKind};
use chrono::Utc;

fn event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

#[test]
fn include_filter_passes_matching_events() {
    let f = EventFilter::include_kinds(&["assistant_message"]);
    let e = event(AgentEventKind::AssistantMessage {
        text: "hello".into(),
    });
    assert!(f.matches(&e));
}

#[test]
fn include_filter_rejects_non_matching_events() {
    let f = EventFilter::include_kinds(&["assistant_message"]);
    let e = event(AgentEventKind::ToolCall {
        tool_name: "read".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: serde_json::json!({}),
    });
    assert!(!f.matches(&e));
}

#[test]
fn exclude_filter_passes_non_matching_events() {
    let f = EventFilter::exclude_kinds(&["warning", "error"]);
    let e = event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    assert!(f.matches(&e));
}

#[test]
fn exclude_filter_rejects_matching_events() {
    let f = EventFilter::exclude_kinds(&["warning", "error"]);
    let e = event(AgentEventKind::Warning {
        message: "uh oh".into(),
    });
    assert!(!f.matches(&e));
}

#[test]
fn empty_include_passes_nothing() {
    let f = EventFilter::include_kinds(&[]);
    let e = event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    assert!(!f.matches(&e));
}

#[test]
fn empty_exclude_passes_everything() {
    let f = EventFilter::exclude_kinds(&[]);
    let e = event(AgentEventKind::Error {
        message: "bad".into(),
        error_code: None,
    });
    assert!(f.matches(&e));
}

#[test]
fn filter_is_case_insensitive() {
    let f = EventFilter::include_kinds(&["Run_Started", "ASSISTANT_MESSAGE"]);
    assert!(f.matches(&event(AgentEventKind::RunStarted {
        message: "go".into(),
    })));
    assert!(f.matches(&event(AgentEventKind::AssistantMessage {
        text: "hi".into(),
    })));
    assert!(!f.matches(&event(AgentEventKind::Error {
        message: "no".into(),
        error_code: None,
    })));
}
