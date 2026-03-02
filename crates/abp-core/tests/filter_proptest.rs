// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property tests for [`abp_core::filter::EventFilter`].

use abp_core::filter::EventFilter;
use abp_core::{AgentEvent, AgentEventKind};
use chrono::{TimeZone, Utc};
use proptest::prelude::*;

// ── Helpers ─────────────────────────────────────────────────────────

/// All known serde tag names for [`AgentEventKind`] variants.
const ALL_KIND_NAMES: &[&str] = &[
    "run_started",
    "run_completed",
    "assistant_delta",
    "assistant_message",
    "tool_call",
    "tool_result",
    "file_changed",
    "command_executed",
    "warning",
    "error",
];

fn event_at(ts: chrono::DateTime<Utc>, kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts,
        kind,
        ext: None,
    }
}

fn arb_datetime() -> impl Strategy<Value = chrono::DateTime<Utc>> {
    (0i64..2_000_000_000).prop_map(|secs| Utc.timestamp_opt(secs, 0).unwrap())
}

/// Strategy producing every `AgentEventKind` variant (all 10).
fn arb_agent_event_kind() -> impl Strategy<Value = AgentEventKind> {
    prop_oneof![
        ".*".prop_map(|message| AgentEventKind::RunStarted { message }),
        ".*".prop_map(|message| AgentEventKind::RunCompleted { message }),
        ".*".prop_map(|text| AgentEventKind::AssistantDelta { text }),
        ".*".prop_map(|text| AgentEventKind::AssistantMessage { text }),
        (".*", ".*").prop_map(|(tool_name, tool_use_id)| AgentEventKind::ToolCall {
            tool_name,
            tool_use_id: Some(tool_use_id),
            parent_tool_use_id: None,
            input: serde_json::json!({}),
        }),
        (".*", ".*").prop_map(|(tool_name, tool_use_id)| AgentEventKind::ToolResult {
            tool_name,
            tool_use_id: Some(tool_use_id),
            output: serde_json::json!(null),
            is_error: false,
        }),
        (".*", ".*").prop_map(|(path, summary)| AgentEventKind::FileChanged { path, summary }),
        ".*".prop_map(|command| AgentEventKind::CommandExecuted {
            command,
            exit_code: Some(0),
            output_preview: None,
        }),
        ".*".prop_map(|message| AgentEventKind::Warning { message }),
        ".*".prop_map(|message| AgentEventKind::Error {
            message,
            error_code: None
        }),
    ]
}

fn arb_agent_event() -> impl Strategy<Value = AgentEvent> {
    (arb_datetime(), arb_agent_event_kind()).prop_map(|(ts, kind)| event_at(ts, kind))
}

/// Pick a random serde kind name from the known list.
fn arb_kind_name() -> impl Strategy<Value = &'static str> {
    prop::sample::select(ALL_KIND_NAMES)
}

// ── Property tests ──────────────────────────────────────────────────

proptest! {
    /// Include filter with a single kind always matches events of that kind.
    #[test]
    fn include_single_kind_always_matches(
        name in arb_kind_name(),
        ts in arb_datetime(),
    ) {
        let filter = EventFilter::include_kinds(&[name]);
        let event = event_at(ts, kind_from_name(name));
        prop_assert!(filter.matches(&event));
    }

    /// Exclude filter with a single kind never matches events of that kind.
    #[test]
    fn exclude_single_kind_never_matches(
        name in arb_kind_name(),
        ts in arb_datetime(),
    ) {
        let filter = EventFilter::exclude_kinds(&[name]);
        let event = event_at(ts, kind_from_name(name));
        prop_assert!(!filter.matches(&event));
    }

    /// When a kind appears in both an include and exclude filter, the
    /// exclude filter rejects it — demonstrating that using both filters
    /// together means exclude wins.
    #[test]
    fn include_and_exclude_same_kind_passes_nothing(
        name in arb_kind_name(),
        ts in arb_datetime(),
    ) {
        let include = EventFilter::include_kinds(&[name]);
        let exclude = EventFilter::exclude_kinds(&[name]);
        let event = event_at(ts, kind_from_name(name));
        // If a caller chains include then exclude, the event is rejected.
        let passes_both = include.matches(&event) && exclude.matches(&event);
        prop_assert!(!passes_both, "exclude should override include for the same kind");
    }

    /// Filtering is deterministic: same filter + same event → same result.
    #[test]
    fn filter_is_deterministic(
        event in arb_agent_event(),
        names in prop::collection::vec(arb_kind_name(), 0..5),
    ) {
        let names_ref: Vec<&str> = names.to_vec();
        let include = EventFilter::include_kinds(&names_ref);
        let exclude = EventFilter::exclude_kinds(&names_ref);

        let r1 = include.matches(&event);
        let r2 = include.matches(&event);
        prop_assert_eq!(r1, r2, "include filter must be deterministic");

        let r3 = exclude.matches(&event);
        let r4 = exclude.matches(&event);
        prop_assert_eq!(r3, r4, "exclude filter must be deterministic");
    }

    /// Every known `AgentEventKind` variant is matchable by its serde name
    /// when used in an include filter.
    #[test]
    fn all_variants_matchable_by_serde_name(
        idx in 0..ALL_KIND_NAMES.len(),
        ts in arb_datetime(),
    ) {
        let name = ALL_KIND_NAMES[idx];
        let kind = kind_from_name(name);
        let event = event_at(ts, kind);
        let filter = EventFilter::include_kinds(&[name]);
        prop_assert!(
            filter.matches(&event),
            "variant {:?} should match serde name {:?}",
            event.kind,
            name,
        );
    }
}

// ── Helpers: construct a representative `AgentEventKind` from name ──

fn kind_from_name(name: &str) -> AgentEventKind {
    match name {
        "run_started" => AgentEventKind::RunStarted {
            message: String::new(),
        },
        "run_completed" => AgentEventKind::RunCompleted {
            message: String::new(),
        },
        "assistant_delta" => AgentEventKind::AssistantDelta {
            text: String::new(),
        },
        "assistant_message" => AgentEventKind::AssistantMessage {
            text: String::new(),
        },
        "tool_call" => AgentEventKind::ToolCall {
            tool_name: String::new(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: serde_json::json!({}),
        },
        "tool_result" => AgentEventKind::ToolResult {
            tool_name: String::new(),
            tool_use_id: None,
            output: serde_json::json!(null),
            is_error: false,
        },
        "file_changed" => AgentEventKind::FileChanged {
            path: String::new(),
            summary: String::new(),
        },
        "command_executed" => AgentEventKind::CommandExecuted {
            command: String::new(),
            exit_code: None,
            output_preview: None,
        },
        "warning" => AgentEventKind::Warning {
            message: String::new(),
        },
        "error" => AgentEventKind::Error {
            message: String::new(),
            error_code: None,
        },
        other => panic!("unknown kind name: {other}"),
    }
}
