// SPDX-License-Identifier: MIT OR Apache-2.0

//! Property tests for [`WorkOrderBuilder`] and [`ReceiptBuilder`].

use abp_core::*;
use chrono::{TimeZone, Utc};
use proptest::prelude::*;

// ── Strategies ──────────────────────────────────────────────────────

fn arb_datetime() -> impl Strategy<Value = chrono::DateTime<Utc>> {
    (0i64..2_000_000_000).prop_map(|secs| Utc.timestamp_opt(secs, 0).unwrap())
}

fn arb_ordered_datetimes() -> impl Strategy<Value = (chrono::DateTime<Utc>, chrono::DateTime<Utc>)>
{
    (0i64..2_000_000_000, 0u32..100_000).prop_map(|(start_secs, delta)| {
        let start = Utc.timestamp_opt(start_secs, 0).unwrap();
        let end = Utc.timestamp_opt(start_secs + i64::from(delta), 0).unwrap();
        (start, end)
    })
}

fn arb_execution_lane() -> impl Strategy<Value = ExecutionLane> {
    prop_oneof![
        Just(ExecutionLane::PatchFirst),
        Just(ExecutionLane::WorkspaceFirst),
    ]
}

fn arb_workspace_mode() -> impl Strategy<Value = WorkspaceMode> {
    prop_oneof![
        Just(WorkspaceMode::PassThrough),
        Just(WorkspaceMode::Staged),
    ]
}

fn arb_outcome() -> impl Strategy<Value = Outcome> {
    prop_oneof![
        Just(Outcome::Complete),
        Just(Outcome::Partial),
        Just(Outcome::Failed),
    ]
}

fn arb_agent_event_kind() -> impl Strategy<Value = AgentEventKind> {
    prop_oneof![
        ".*".prop_map(|message| AgentEventKind::RunStarted { message }),
        ".*".prop_map(|message| AgentEventKind::RunCompleted { message }),
        ".*".prop_map(|text| AgentEventKind::AssistantDelta { text }),
        ".*".prop_map(|text| AgentEventKind::AssistantMessage { text }),
        ".*".prop_map(|message| AgentEventKind::Warning { message }),
        ".*".prop_map(|message| AgentEventKind::Error { message }),
    ]
}

fn arb_agent_event() -> impl Strategy<Value = AgentEvent> {
    (arb_datetime(), arb_agent_event_kind()).prop_map(|(ts, kind)| AgentEvent {
        ts,
        kind,
        ext: None,
    })
}

// ── WorkOrderBuilder property tests ─────────────────────────────────

proptest! {
    /// Any task string produces a valid WorkOrder via the builder.
    #[test]
    fn any_task_produces_valid_work_order(task in ".*") {
        let wo = WorkOrderBuilder::new(task.clone()).build();
        prop_assert_eq!(&wo.task, &task);
        prop_assert!(!wo.id.is_nil());
    }

    /// Built WorkOrder always serializes to valid JSON.
    #[test]
    fn work_order_serializes_to_valid_json(task in ".*") {
        let wo = WorkOrderBuilder::new(task).build();
        let json = serde_json::to_string(&wo);
        prop_assert!(json.is_ok(), "serialization failed: {:?}", json.err());
        let parsed: serde_json::Value = serde_json::from_str(&json.unwrap()).unwrap();
        prop_assert!(parsed.is_object());
    }

    /// Built WorkOrder always round-trips through serde.
    #[test]
    fn work_order_serde_round_trip(task in ".*") {
        let wo = WorkOrderBuilder::new(task).build();
        let json = serde_json::to_string(&wo).unwrap();
        let deser: WorkOrder = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&deser).unwrap();
        prop_assert_eq!(json, json2);
    }

    /// Builder with all fields set produces a complete WorkOrder.
    #[test]
    fn work_order_builder_all_fields(
        task in ".*",
        root in ".*",
        model in ".*",
        lane in arb_execution_lane(),
        ws_mode in arb_workspace_mode(),
        max_turns in 1u32..1000,
        budget in 0.01f64..10000.0,
    ) {
        let include = vec!["*.rs".to_string()];
        let exclude = vec!["target/**".to_string()];
        let wo = WorkOrderBuilder::new(task.clone())
            .lane(lane)
            .root(root.clone())
            .workspace_mode(ws_mode)
            .include(include.clone())
            .exclude(exclude.clone())
            .model(model.clone())
            .max_turns(max_turns)
            .max_budget_usd(budget)
            .build();

        prop_assert_eq!(&wo.task, &task);
        prop_assert_eq!(&wo.workspace.root, &root);
        prop_assert_eq!(wo.config.model.as_deref(), Some(model.as_str()));
        prop_assert_eq!(wo.config.max_turns, Some(max_turns));
        prop_assert_eq!(wo.config.max_budget_usd, Some(budget));
        prop_assert_eq!(&wo.workspace.include, &include);
        prop_assert_eq!(&wo.workspace.exclude, &exclude);
        // Verify it still serializes cleanly.
        let json = serde_json::to_string(&wo);
        prop_assert!(json.is_ok());
    }

    /// Default builder fields are valid.
    #[test]
    fn work_order_default_fields_valid(task in ".*") {
        let wo = WorkOrderBuilder::new(task).build();
        prop_assert_eq!(&wo.workspace.root, ".");
        prop_assert!(wo.config.model.is_none());
        prop_assert!(wo.config.max_turns.is_none());
        prop_assert!(wo.config.max_budget_usd.is_none());
        prop_assert!(wo.context.files.is_empty());
        prop_assert!(wo.context.snippets.is_empty());
        prop_assert!(wo.policy.allowed_tools.is_empty());
        prop_assert!(wo.requirements.required.is_empty());
    }
}

// ── ReceiptBuilder property tests ───────────────────────────────────

proptest! {
    /// Built receipt always serializes to valid JSON.
    #[test]
    fn receipt_serializes_to_valid_json(
        backend_id in "[a-zA-Z0-9_-]{1,32}",
        outcome in arb_outcome(),
    ) {
        let receipt = ReceiptBuilder::new(backend_id)
            .outcome(outcome)
            .build();
        let json = serde_json::to_string(&receipt);
        prop_assert!(json.is_ok(), "serialization failed: {:?}", json.err());
        let parsed: serde_json::Value = serde_json::from_str(&json.unwrap()).unwrap();
        prop_assert!(parsed.is_object());
    }

    /// Receipt with hash always passes hash verification.
    #[test]
    fn receipt_with_hash_passes_verification(
        backend_id in "[a-zA-Z0-9_-]{1,32}",
        outcome in arb_outcome(),
    ) {
        let receipt = ReceiptBuilder::new(backend_id)
            .outcome(outcome)
            .with_hash()
            .expect("hashing should succeed");

        // Verify the hash is present and correct.
        prop_assert!(receipt.receipt_sha256.is_some());
        let stored_hash = receipt.receipt_sha256.as_ref().unwrap();
        prop_assert_eq!(stored_hash.len(), 64);

        // Recompute and compare.
        let recomputed = receipt_hash(&receipt).unwrap();
        prop_assert_eq!(stored_hash, &recomputed);
    }

    /// Receipt with timestamp ordering is valid (duration_ms computed correctly).
    #[test]
    fn receipt_timestamp_ordering(
        backend_id in "[a-zA-Z0-9_-]{1,32}",
        (start, end) in arb_ordered_datetimes(),
    ) {
        let receipt = ReceiptBuilder::new(backend_id)
            .started_at(start)
            .finished_at(end)
            .build();

        prop_assert!(receipt.meta.started_at <= receipt.meta.finished_at);
        let expected_ms = (end - start).num_milliseconds().max(0) as u64;
        prop_assert_eq!(receipt.meta.duration_ms, expected_ms);
    }

    /// Built receipt round-trips through serde.
    #[test]
    fn receipt_serde_round_trip(
        backend_id in "[a-zA-Z0-9_-]{1,32}",
        outcome in arb_outcome(),
    ) {
        let receipt = ReceiptBuilder::new(backend_id)
            .outcome(outcome)
            .build();
        let json = serde_json::to_string(&receipt).unwrap();
        let deser: Receipt = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&deser).unwrap();
        prop_assert_eq!(json, json2);
    }

    /// Builder with trace events preserves count.
    #[test]
    fn receipt_trace_events_preserve_count(
        backend_id in "[a-zA-Z0-9_-]{1,32}",
        events in prop::collection::vec(arb_agent_event(), 0..10),
    ) {
        let expected_count = events.len();
        let mut builder = ReceiptBuilder::new(backend_id);
        for event in events {
            builder = builder.add_trace_event(event);
        }
        let receipt = builder.build();
        prop_assert_eq!(receipt.trace.len(), expected_count);
    }
}
