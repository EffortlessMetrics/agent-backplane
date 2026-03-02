// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive deep tests for MockBackend: construction, run behavior,
//! receipt generation, trait dispatch, event streaming, identity, capabilities,
//! sequential runs, WorkOrder configurations, serde roundtrip, SidecarBackend
//! configuration, and edge cases.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use abp_core::{
    AgentEvent, AgentEventKind, CONTRACT_VERSION, Capability, CapabilityRequirement,
    CapabilityRequirements, ContextPacket, ContextSnippet, ExecutionLane, ExecutionMode,
    MinSupport, Outcome, PolicyProfile, Receipt, RuntimeConfig, SupportLevel, WorkOrder,
    WorkOrderBuilder, WorkspaceMode, WorkspaceSpec,
};
use abp_integrations::{
    Backend, MockBackend, SidecarBackend, ensure_capability_requirements, extract_execution_mode,
    validate_passthrough_compatibility,
};
use serde_json::json;
use tokio::sync::mpsc;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn wo(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task).build()
}

async fn run_mock(task: &str) -> (Receipt, Vec<AgentEvent>) {
    let (tx, mut rx) = mpsc::channel(64);
    let receipt = MockBackend.run(Uuid::new_v4(), wo(task), tx).await.unwrap();
    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    (receipt, events)
}

fn manual_work_order(task: &str) -> WorkOrder {
    WorkOrder {
        id: Uuid::nil(),
        task: task.into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
            mode: WorkspaceMode::PassThrough,
            include: vec![],
            exclude: vec![],
        },
        context: ContextPacket::default(),
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig::default(),
    }
}

// ===========================================================================
// Section 1: MockBackend construction (tests 1–5)
// ===========================================================================

#[test]
fn t01_mock_backend_is_unit_struct() {
    let _b = MockBackend;
}

#[test]
fn t02_mock_backend_clone() {
    let a = MockBackend;
    let b = a.clone();
    assert_eq!(a.identity().id, b.identity().id);
}

#[test]
fn t03_mock_backend_debug_impl() {
    let dbg = format!("{:?}", MockBackend);
    assert!(dbg.contains("MockBackend"));
}

#[test]
fn t04_mock_backend_multiple_instances_same_identity() {
    let ids: Vec<_> = (0..5).map(|_| MockBackend.identity().id).collect();
    assert!(ids.iter().all(|id| id == "mock"));
}

#[test]
fn t05_mock_backend_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MockBackend>();
}

// ===========================================================================
// Section 2: MockBackend::run() returns expected events (tests 6–15)
// ===========================================================================

#[tokio::test]
async fn t06_run_returns_ok() {
    let (tx, _rx) = mpsc::channel(16);
    let result = MockBackend.run(Uuid::new_v4(), wo("hello"), tx).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn t07_run_emits_exactly_four_events() {
    let (_receipt, events) = run_mock("test").await;
    assert_eq!(events.len(), 4);
}

#[tokio::test]
async fn t08_first_event_is_run_started() {
    let (_receipt, events) = run_mock("test").await;
    assert!(matches!(events[0].kind, AgentEventKind::RunStarted { .. }));
}

#[tokio::test]
async fn t09_last_event_is_run_completed() {
    let (_receipt, events) = run_mock("test").await;
    assert!(matches!(
        events.last().unwrap().kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[tokio::test]
async fn t10_middle_events_are_assistant_messages() {
    let (_receipt, events) = run_mock("test").await;
    assert!(matches!(
        events[1].kind,
        AgentEventKind::AssistantMessage { .. }
    ));
    assert!(matches!(
        events[2].kind,
        AgentEventKind::AssistantMessage { .. }
    ));
}

#[tokio::test]
async fn t11_run_started_message_contains_task() {
    let (_receipt, events) = run_mock("refactor widgets").await;
    if let AgentEventKind::RunStarted { message } = &events[0].kind {
        assert!(message.contains("refactor widgets"));
    } else {
        panic!("expected RunStarted");
    }
}

#[tokio::test]
async fn t12_run_completed_message_is_mock_run_complete() {
    let (_receipt, events) = run_mock("test").await;
    if let AgentEventKind::RunCompleted { message } = &events.last().unwrap().kind {
        assert_eq!(message, "mock run complete");
    } else {
        panic!("expected RunCompleted");
    }
}

#[tokio::test]
async fn t13_assistant_message_mentions_mock() {
    let (_receipt, events) = run_mock("test").await;
    if let AgentEventKind::AssistantMessage { text } = &events[1].kind {
        assert!(text.contains("mock backend"));
    } else {
        panic!("expected AssistantMessage");
    }
}

#[tokio::test]
async fn t14_assistant_message_mentions_sidecar() {
    let (_receipt, events) = run_mock("test").await;
    if let AgentEventKind::AssistantMessage { text } = &events[2].kind {
        assert!(text.contains("sidecar"));
    } else {
        panic!("expected AssistantMessage");
    }
}

#[tokio::test]
async fn t15_all_events_have_none_ext() {
    let (_receipt, events) = run_mock("test").await;
    for (i, ev) in events.iter().enumerate() {
        assert!(ev.ext.is_none(), "event {i} should have ext = None");
    }
}

// ===========================================================================
// Section 3: Receipt generation (tests 16–30)
// ===========================================================================

#[tokio::test]
async fn t16_receipt_outcome_is_complete() {
    let (receipt, _) = run_mock("test").await;
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t17_receipt_has_sha256_hash() {
    let (receipt, _) = run_mock("test").await;
    assert!(receipt.receipt_sha256.is_some());
}

#[tokio::test]
async fn t18_receipt_hash_is_64_hex_chars() {
    let (receipt, _) = run_mock("test").await;
    let hash = receipt.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[tokio::test]
async fn t19_receipt_hash_recomputable() {
    let (receipt, _) = run_mock("test").await;
    let stored = receipt.receipt_sha256.clone().unwrap();
    let recomputed = abp_core::receipt_hash(&receipt).unwrap();
    assert_eq!(stored, recomputed);
}

#[tokio::test]
async fn t20_receipt_hash_deterministic() {
    let (receipt, _) = run_mock("test").await;
    let h1 = abp_core::receipt_hash(&receipt).unwrap();
    let h2 = abp_core::receipt_hash(&receipt).unwrap();
    assert_eq!(h1, h2);
}

#[tokio::test]
async fn t21_receipt_preserves_run_id() {
    let run_id = Uuid::new_v4();
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend.run(run_id, wo("test"), tx).await.unwrap();
    assert_eq!(receipt.meta.run_id, run_id);
}

#[tokio::test]
async fn t22_receipt_preserves_work_order_id() {
    let work_order = wo("test");
    let wo_id = work_order.id;
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend
        .run(Uuid::new_v4(), work_order, tx)
        .await
        .unwrap();
    assert_eq!(receipt.meta.work_order_id, wo_id);
}

#[tokio::test]
async fn t23_receipt_contract_version() {
    let (receipt, _) = run_mock("test").await;
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    assert_eq!(receipt.meta.contract_version, "abp/v0.1");
}

#[tokio::test]
async fn t24_receipt_timing_finished_gte_started() {
    let (receipt, _) = run_mock("test").await;
    assert!(receipt.meta.finished_at >= receipt.meta.started_at);
}

#[tokio::test]
async fn t25_receipt_duration_consistent_with_timestamps() {
    let (receipt, _) = run_mock("test").await;
    let delta = (receipt.meta.finished_at - receipt.meta.started_at)
        .to_std()
        .unwrap_or_default()
        .as_millis() as u64;
    assert_eq!(receipt.meta.duration_ms, delta);
}

#[tokio::test]
async fn t26_receipt_duration_under_30s() {
    let (receipt, _) = run_mock("test").await;
    assert!(receipt.meta.duration_ms < 30_000);
}

#[tokio::test]
async fn t27_receipt_usage_raw_contains_note() {
    let (receipt, _) = run_mock("test").await;
    assert_eq!(receipt.usage_raw, json!({"note": "mock"}));
}

#[tokio::test]
async fn t28_receipt_usage_normalized_zeros() {
    let (receipt, _) = run_mock("test").await;
    assert_eq!(receipt.usage.input_tokens, Some(0));
    assert_eq!(receipt.usage.output_tokens, Some(0));
    assert_eq!(receipt.usage.estimated_cost_usd, Some(0.0));
}

#[tokio::test]
async fn t29_receipt_verification_report() {
    let (receipt, _) = run_mock("test").await;
    assert!(receipt.verification.harness_ok);
    assert!(receipt.verification.git_diff.is_none());
    assert!(receipt.verification.git_status.is_none());
}

#[tokio::test]
async fn t30_receipt_artifacts_empty() {
    let (receipt, _) = run_mock("test").await;
    assert!(receipt.artifacts.is_empty());
}

// ===========================================================================
// Section 4: Backend trait object dispatch (tests 31–40)
// ===========================================================================

#[test]
fn t31_backend_is_object_safe_box() {
    let _: Box<dyn Backend> = Box::new(MockBackend);
}

#[test]
fn t32_backend_is_object_safe_arc() {
    let _: Arc<dyn Backend> = Arc::new(MockBackend);
}

#[tokio::test]
async fn t33_boxed_backend_run_ok() {
    let backend: Box<dyn Backend> = Box::new(MockBackend);
    let (tx, _rx) = mpsc::channel(16);
    let receipt = backend.run(Uuid::new_v4(), wo("boxed"), tx).await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t34_arc_backend_run_ok() {
    let backend: Arc<dyn Backend> = Arc::new(MockBackend);
    let (tx, _rx) = mpsc::channel(16);
    let receipt = backend.run(Uuid::new_v4(), wo("arc"), tx).await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[test]
fn t35_dyn_identity() {
    let backend: Box<dyn Backend> = Box::new(MockBackend);
    assert_eq!(backend.identity().id, "mock");
}

#[test]
fn t36_dyn_capabilities() {
    let backend: Box<dyn Backend> = Box::new(MockBackend);
    assert!(!backend.capabilities().is_empty());
}

#[tokio::test]
async fn t37_arc_cloned_across_tasks() {
    let backend: Arc<dyn Backend> = Arc::new(MockBackend);
    let b1 = Arc::clone(&backend);
    let b2 = Arc::clone(&backend);
    let h1 = tokio::spawn(async move {
        let (tx, _rx) = mpsc::channel(16);
        b1.run(Uuid::new_v4(), wo("t1"), tx).await.unwrap()
    });
    let h2 = tokio::spawn(async move {
        let (tx, _rx) = mpsc::channel(16);
        b2.run(Uuid::new_v4(), wo("t2"), tx).await.unwrap()
    });
    let r1 = h1.await.unwrap();
    let r2 = h2.await.unwrap();
    assert_ne!(r1.meta.run_id, r2.meta.run_id);
}

#[tokio::test]
async fn t38_backend_vec_heterogeneous() {
    let backends: Vec<Box<dyn Backend>> = vec![Box::new(MockBackend), Box::new(MockBackend)];
    for b in &backends {
        assert_eq!(b.identity().id, "mock");
    }
}

#[tokio::test]
async fn t39_hashmap_registry_lookup() {
    let mut reg: std::collections::HashMap<String, Box<dyn Backend>> =
        std::collections::HashMap::new();
    reg.insert("mock".into(), Box::new(MockBackend));
    assert!(reg.contains_key("mock"));
    assert_eq!(reg.get("mock").unwrap().identity().id, "mock");
}

#[tokio::test]
async fn t40_btreemap_registry_ordered() {
    let mut reg: BTreeMap<String, Arc<dyn Backend>> = BTreeMap::new();
    reg.insert("a-mock".into(), Arc::new(MockBackend));
    reg.insert("b-mock".into(), Arc::new(MockBackend));
    let keys: Vec<_> = reg.keys().collect();
    assert_eq!(keys, vec!["a-mock", "b-mock"]);
}

// ===========================================================================
// Section 5: Event streaming behavior (tests 41–50)
// ===========================================================================

#[tokio::test]
async fn t41_events_have_monotonic_timestamps() {
    let (_receipt, events) = run_mock("test").await;
    for w in events.windows(2) {
        assert!(w[1].ts >= w[0].ts);
    }
}

#[tokio::test]
async fn t42_trace_length_matches_stream_length() {
    let (receipt, events) = run_mock("test").await;
    assert_eq!(receipt.trace.len(), events.len());
}

#[tokio::test]
async fn t43_trace_event_kinds_match_stream() {
    let (receipt, events) = run_mock("test").await;
    for (i, (t, s)) in receipt.trace.iter().zip(&events).enumerate() {
        assert_eq!(
            std::mem::discriminant(&t.kind),
            std::mem::discriminant(&s.kind),
            "event {i} kind mismatch"
        );
    }
}

#[tokio::test]
async fn t44_small_channel_capacity_with_consumer() {
    let (tx, mut rx) = mpsc::channel(1);
    let consumer = tokio::spawn(async move {
        let mut n = 0u32;
        while rx.recv().await.is_some() {
            n += 1;
        }
        n
    });
    let receipt = MockBackend
        .run(Uuid::new_v4(), wo("test"), tx)
        .await
        .unwrap();
    drop(receipt);
    let count = consumer.await.unwrap();
    assert_eq!(count, 4);
}

#[tokio::test]
async fn t45_large_channel_capacity() {
    let (tx, mut rx) = mpsc::channel(1024);
    let _receipt = MockBackend
        .run(Uuid::new_v4(), wo("test"), tx)
        .await
        .unwrap();
    let mut count = 0u32;
    while rx.try_recv().is_ok() {
        count += 1;
    }
    assert_eq!(count, 4);
}

#[tokio::test]
async fn t46_dropped_receiver_run_still_completes() {
    let (tx, rx) = mpsc::channel(16);
    drop(rx);
    // run should still complete even if receiver is dropped
    let result = MockBackend.run(Uuid::new_v4(), wo("test"), tx).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn t47_trace_events_have_none_ext() {
    let (receipt, _) = run_mock("test").await;
    for ev in &receipt.trace {
        assert!(ev.ext.is_none());
    }
}

#[tokio::test]
async fn t48_events_contain_no_tool_calls() {
    let (_receipt, events) = run_mock("test").await;
    for ev in &events {
        assert!(
            !matches!(ev.kind, AgentEventKind::ToolCall { .. }),
            "mock should not emit ToolCall events"
        );
    }
}

#[tokio::test]
async fn t49_events_contain_no_errors() {
    let (_receipt, events) = run_mock("test").await;
    for ev in &events {
        assert!(
            !matches!(ev.kind, AgentEventKind::Error { .. }),
            "mock should not emit Error events"
        );
    }
}

#[tokio::test]
async fn t50_events_contain_no_warnings() {
    let (_receipt, events) = run_mock("test").await;
    for ev in &events {
        assert!(
            !matches!(ev.kind, AgentEventKind::Warning { .. }),
            "mock should not emit Warning events"
        );
    }
}

// ===========================================================================
// Section 6: Backend identity and capabilities (tests 51–60)
// ===========================================================================

#[test]
fn t51_identity_id_is_mock() {
    assert_eq!(MockBackend.identity().id, "mock");
}

#[test]
fn t52_identity_backend_version() {
    assert_eq!(
        MockBackend.identity().backend_version.as_deref(),
        Some("0.1")
    );
}

#[test]
fn t53_identity_adapter_version() {
    assert_eq!(
        MockBackend.identity().adapter_version.as_deref(),
        Some("0.1")
    );
}

#[test]
fn t54_capabilities_count_is_six() {
    assert_eq!(MockBackend.capabilities().len(), 6);
}

#[test]
fn t55_streaming_is_native() {
    assert!(matches!(
        MockBackend.capabilities().get(&Capability::Streaming),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn t56_tool_read_is_emulated() {
    assert!(matches!(
        MockBackend.capabilities().get(&Capability::ToolRead),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn t57_tool_write_is_emulated() {
    assert!(matches!(
        MockBackend.capabilities().get(&Capability::ToolWrite),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn t58_tool_edit_is_emulated() {
    assert!(matches!(
        MockBackend.capabilities().get(&Capability::ToolEdit),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn t59_tool_bash_is_emulated() {
    assert!(matches!(
        MockBackend.capabilities().get(&Capability::ToolBash),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn t60_structured_output_is_emulated() {
    assert!(matches!(
        MockBackend
            .capabilities()
            .get(&Capability::StructuredOutputJsonSchema),
        Some(SupportLevel::Emulated)
    ));
}

// ===========================================================================
// Section 7: Multiple sequential mock runs (tests 61–65)
// ===========================================================================

#[tokio::test]
async fn t61_sequential_runs_produce_unique_run_ids() {
    let mut ids = HashSet::new();
    for i in 0..10 {
        let (receipt, _) = run_mock(&format!("seq {i}")).await;
        assert!(ids.insert(receipt.meta.run_id));
    }
    assert_eq!(ids.len(), 10);
}

#[tokio::test]
async fn t62_sequential_runs_produce_unique_hashes() {
    let mut hashes = HashSet::new();
    for i in 0..5 {
        let (receipt, _) = run_mock(&format!("hash-seq {i}")).await;
        hashes.insert(receipt.receipt_sha256.clone().unwrap());
    }
    assert_eq!(hashes.len(), 5);
}

#[tokio::test]
async fn t63_sequential_runs_all_complete() {
    for i in 0..5 {
        let (receipt, _) = run_mock(&format!("complete-seq {i}")).await;
        assert_eq!(receipt.outcome, Outcome::Complete);
    }
}

#[tokio::test]
async fn t64_concurrent_runs_independent_receipts() {
    let handles: Vec<_> = (0..8)
        .map(|i| {
            tokio::spawn(async move {
                let (tx, _rx) = mpsc::channel(16);
                MockBackend
                    .run(Uuid::new_v4(), wo(&format!("concurrent {i}")), tx)
                    .await
                    .unwrap()
            })
        })
        .collect();

    let mut ids = HashSet::new();
    for h in handles {
        let receipt = h.await.unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
        assert!(ids.insert(receipt.meta.run_id));
    }
    assert_eq!(ids.len(), 8);
}

#[tokio::test]
async fn t65_same_task_produces_different_hashes_due_to_time() {
    let (r1, _) = run_mock("same task").await;
    let (r2, _) = run_mock("same task").await;
    // UUIDs and timestamps differ, so hashes should differ
    assert_ne!(r1.receipt_sha256, r2.receipt_sha256);
}

// ===========================================================================
// Section 8: WorkOrder configurations (tests 66–75)
// ===========================================================================

#[tokio::test]
async fn t66_default_execution_mode_is_mapped() {
    let (receipt, _) = run_mock("test").await;
    assert_eq!(receipt.mode, ExecutionMode::Mapped);
}

#[tokio::test]
async fn t67_passthrough_mode_propagates() {
    let mut work_order = wo("passthrough test");
    work_order
        .config
        .vendor
        .insert("abp".into(), json!({"mode": "passthrough"}));
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend
        .run(Uuid::new_v4(), work_order, tx)
        .await
        .unwrap();
    assert_eq!(receipt.mode, ExecutionMode::Passthrough);
}

#[tokio::test]
async fn t68_dotted_abp_mode_key() {
    let mut work_order = wo("dotted mode");
    work_order
        .config
        .vendor
        .insert("abp.mode".into(), json!("passthrough"));
    let mode = extract_execution_mode(&work_order);
    assert_eq!(mode, ExecutionMode::Passthrough);
}

#[tokio::test]
async fn t69_nested_mode_priority_over_dotted() {
    let mut work_order = wo("priority");
    work_order
        .config
        .vendor
        .insert("abp".into(), json!({"mode": "passthrough"}));
    work_order
        .config
        .vendor
        .insert("abp.mode".into(), json!("mapped"));
    assert_eq!(
        extract_execution_mode(&work_order),
        ExecutionMode::Passthrough
    );
}

#[tokio::test]
async fn t70_invalid_mode_falls_back_to_mapped() {
    let mut work_order = wo("invalid");
    work_order
        .config
        .vendor
        .insert("abp".into(), json!({"mode": "nonexistent"}));
    assert_eq!(extract_execution_mode(&work_order), ExecutionMode::Mapped);
}

#[tokio::test]
async fn t71_workspace_first_lane() {
    let work_order = WorkOrderBuilder::new("test")
        .lane(ExecutionLane::WorkspaceFirst)
        .build();
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend
        .run(Uuid::new_v4(), work_order, tx)
        .await
        .unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t72_custom_model_config() {
    let work_order = WorkOrderBuilder::new("model test")
        .model("gpt-4-turbo")
        .build();
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend
        .run(Uuid::new_v4(), work_order, tx)
        .await
        .unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t73_max_turns_config() {
    let work_order = WorkOrderBuilder::new("turns test").max_turns(5).build();
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend
        .run(Uuid::new_v4(), work_order, tx)
        .await
        .unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t74_max_budget_config() {
    let work_order = WorkOrderBuilder::new("budget test")
        .max_budget_usd(10.0)
        .build();
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend
        .run(Uuid::new_v4(), work_order, tx)
        .await
        .unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t75_context_packet_with_files_and_snippets() {
    let ctx = ContextPacket {
        files: vec!["src/main.rs".into(), "Cargo.toml".into()],
        snippets: vec![ContextSnippet {
            name: "hint".into(),
            content: "use async".into(),
        }],
    };
    let work_order = WorkOrderBuilder::new("context test").context(ctx).build();
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend
        .run(Uuid::new_v4(), work_order, tx)
        .await
        .unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

// ===========================================================================
// Section 9: Backend serde roundtrip (tests 76–80)
// ===========================================================================

#[tokio::test]
async fn t76_receipt_serde_roundtrip() {
    let (receipt, _) = run_mock("serde").await;
    let json = serde_json::to_string(&receipt).unwrap();
    let de: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(de.meta.run_id, receipt.meta.run_id);
    assert_eq!(de.meta.work_order_id, receipt.meta.work_order_id);
    assert_eq!(de.backend.id, receipt.backend.id);
    assert_eq!(de.outcome, receipt.outcome);
    assert_eq!(de.receipt_sha256, receipt.receipt_sha256);
    assert_eq!(de.mode, receipt.mode);
    assert_eq!(de.trace.len(), receipt.trace.len());
}

#[tokio::test]
async fn t77_receipt_pretty_json_roundtrip() {
    let (receipt, _) = run_mock("pretty serde").await;
    let json = serde_json::to_string_pretty(&receipt).unwrap();
    let de: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(de.receipt_sha256, receipt.receipt_sha256);
}

#[tokio::test]
async fn t78_receipt_value_roundtrip() {
    let (receipt, _) = run_mock("value roundtrip").await;
    let val = serde_json::to_value(&receipt).unwrap();
    let de: Receipt = serde_json::from_value(val).unwrap();
    assert_eq!(de.meta.run_id, receipt.meta.run_id);
}

#[tokio::test]
async fn t79_work_order_serde_roundtrip() {
    let work_order = WorkOrderBuilder::new("serde wo")
        .model("gpt-4")
        .max_turns(3)
        .lane(ExecutionLane::WorkspaceFirst)
        .build();
    let json = serde_json::to_string(&work_order).unwrap();
    let de: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(de.task, work_order.task);
    assert_eq!(de.id, work_order.id);
    assert_eq!(de.config.model, work_order.config.model);
    assert_eq!(de.config.max_turns, work_order.config.max_turns);
}

#[tokio::test]
async fn t80_receipt_hash_stable_after_serde() {
    let (receipt, _) = run_mock("hash stable").await;
    let json = serde_json::to_string(&receipt).unwrap();
    let de: Receipt = serde_json::from_str(&json).unwrap();
    let hash_original = abp_core::receipt_hash(&receipt).unwrap();
    let hash_deserialized = abp_core::receipt_hash(&de).unwrap();
    assert_eq!(hash_original, hash_deserialized);
}

// ===========================================================================
// Section 10: SidecarBackend configuration validation (tests 81–86)
// ===========================================================================

#[test]
fn t81_sidecar_backend_construction() {
    let spec = abp_host::SidecarSpec::new("node");
    let backend = SidecarBackend::new(spec);
    assert_eq!(backend.spec.command, "node");
}

#[test]
fn t82_sidecar_backend_spec_with_args() {
    let mut spec = abp_host::SidecarSpec::new("python3");
    spec.args = vec!["sidecar.py".into()];
    let backend = SidecarBackend::new(spec);
    assert_eq!(backend.spec.args, vec!["sidecar.py"]);
}

#[test]
fn t83_sidecar_backend_spec_with_env() {
    let mut spec = abp_host::SidecarSpec::new("node");
    spec.env.insert("API_KEY".into(), "test-key".into());
    let backend = SidecarBackend::new(spec);
    assert_eq!(
        backend.spec.env.get("API_KEY").map(|s| s.as_str()),
        Some("test-key")
    );
}

#[test]
fn t84_sidecar_backend_spec_with_cwd() {
    let mut spec = abp_host::SidecarSpec::new("node");
    spec.cwd = Some("/tmp/work".into());
    let backend = SidecarBackend::new(spec);
    assert_eq!(backend.spec.cwd.as_deref(), Some("/tmp/work"));
}

#[test]
fn t85_sidecar_backend_identity() {
    let spec = abp_host::SidecarSpec::new("node");
    let backend = SidecarBackend::new(spec);
    let id = backend.identity();
    assert_eq!(id.id, "sidecar");
    assert_eq!(id.adapter_version.as_deref(), Some("0.1"));
}

#[test]
fn t86_sidecar_backend_empty_capabilities() {
    let spec = abp_host::SidecarSpec::new("node");
    let backend = SidecarBackend::new(spec);
    assert!(backend.capabilities().is_empty());
}

// ===========================================================================
// Section 11: Capability requirement checks (tests 87–92)
// ===========================================================================

#[test]
fn t87_empty_requirements_pass() {
    let reqs = CapabilityRequirements::default();
    let caps = MockBackend.capabilities();
    assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
}

#[tokio::test]
async fn t88_native_streaming_requirement_passes() {
    let mut work_order = wo("test");
    work_order.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    let (tx, _rx) = mpsc::channel(16);
    let result = MockBackend.run(Uuid::new_v4(), work_order, tx).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn t89_emulated_tool_read_requirement_passes() {
    let mut work_order = wo("test");
    work_order.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Emulated,
        }],
    };
    let (tx, _rx) = mpsc::channel(16);
    assert!(
        MockBackend
            .run(Uuid::new_v4(), work_order, tx)
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn t90_native_tool_read_requirement_fails() {
    let mut work_order = wo("test");
    work_order.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Native,
        }],
    };
    let (tx, _rx) = mpsc::channel(16);
    assert!(
        MockBackend
            .run(Uuid::new_v4(), work_order, tx)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn t91_missing_capability_rejects_run() {
    let mut work_order = wo("test");
    work_order.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Emulated,
        }],
    };
    let (tx, _rx) = mpsc::channel(16);
    let result = MockBackend.run(Uuid::new_v4(), work_order, tx).await;
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("capability requirements not satisfied")
    );
}

#[tokio::test]
async fn t92_multiple_satisfied_requirements() {
    let mut work_order = wo("multi-cap");
    work_order.requirements = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolBash,
                min_support: MinSupport::Emulated,
            },
            CapabilityRequirement {
                capability: Capability::ToolEdit,
                min_support: MinSupport::Emulated,
            },
        ],
    };
    let (tx, _rx) = mpsc::channel(16);
    assert!(
        MockBackend
            .run(Uuid::new_v4(), work_order, tx)
            .await
            .is_ok()
    );
}

// ===========================================================================
// Section 12: Edge cases (tests 93–103)
// ===========================================================================

#[tokio::test]
async fn t93_empty_task_string() {
    let (receipt, events) = run_mock("").await;
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(events.len(), 4);
    assert!(receipt.receipt_sha256.is_some());
}

#[tokio::test]
async fn t94_very_large_task_string() {
    let large = "Z".repeat(1_000_000);
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend
        .run(Uuid::new_v4(), wo(&large), tx)
        .await
        .unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t95_unicode_task_string() {
    let (receipt, events) = run_mock("こんにちは世界 🌍 émojis").await;
    assert_eq!(receipt.outcome, Outcome::Complete);
    if let AgentEventKind::RunStarted { message } = &events[0].kind {
        assert!(message.contains("こんにちは世界"));
    }
}

#[tokio::test]
async fn t96_newlines_in_task() {
    let (receipt, _) = run_mock("line1\nline2\nline3").await;
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t97_special_chars_in_task() {
    let (receipt, _) = run_mock(r#"task with "quotes" and <angles> & amps"#).await;
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t98_nil_uuid_run_id() {
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend
        .run(Uuid::nil(), wo("nil id"), tx)
        .await
        .unwrap();
    assert_eq!(receipt.meta.run_id, Uuid::nil());
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t99_manual_work_order_nil_id() {
    let work_order = manual_work_order("manual");
    assert_eq!(work_order.id, Uuid::nil());
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend
        .run(Uuid::new_v4(), work_order, tx)
        .await
        .unwrap();
    assert_eq!(receipt.meta.work_order_id, Uuid::nil());
}

#[test]
fn t100_validate_passthrough_compatibility_always_ok() {
    let work_order = wo("test");
    assert!(validate_passthrough_compatibility(&work_order).is_ok());
}

#[tokio::test]
async fn t101_receipt_backend_identity_matches_trait_method() {
    let (receipt, _) = run_mock("identity check").await;
    let expected = MockBackend.identity();
    assert_eq!(receipt.backend.id, expected.id);
    assert_eq!(receipt.backend.backend_version, expected.backend_version);
    assert_eq!(receipt.backend.adapter_version, expected.adapter_version);
}

#[tokio::test]
async fn t102_receipt_capabilities_match_trait_method() {
    let (receipt, _) = run_mock("caps check").await;
    let expected = MockBackend.capabilities();
    assert_eq!(receipt.capabilities.len(), expected.len());
    for (cap, level) in &expected {
        let receipt_level = receipt.capabilities.get(cap).expect("missing capability");
        assert_eq!(
            std::mem::discriminant(receipt_level),
            std::mem::discriminant(level),
            "capability {cap:?} level mismatch"
        );
    }
}

#[tokio::test]
async fn t103_policy_with_restrictions_still_runs() {
    let policy = PolicyProfile {
        allowed_tools: vec!["read".into()],
        disallowed_tools: vec!["bash".into()],
        deny_read: vec!["*.secret".into()],
        deny_write: vec!["/etc/*".into()],
        allow_network: vec![],
        deny_network: vec!["*.evil.com".into()],
        require_approval_for: vec!["write".into()],
    };
    let work_order = WorkOrderBuilder::new("policy test").policy(policy).build();
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend
        .run(Uuid::new_v4(), work_order, tx)
        .await
        .unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

// ===========================================================================
// Section 13: Additional coverage (tests 104–108)
// ===========================================================================

#[test]
fn t104_sidecar_backend_clone() {
    let spec = abp_host::SidecarSpec::new("node");
    let a = SidecarBackend::new(spec);
    let b = a.clone();
    assert_eq!(a.spec.command, b.spec.command);
}

#[test]
fn t105_sidecar_backend_debug() {
    let spec = abp_host::SidecarSpec::new("node");
    let backend = SidecarBackend::new(spec);
    let dbg = format!("{:?}", backend);
    assert!(dbg.contains("SidecarBackend"));
}

#[test]
fn t106_mock_capabilities_no_mcp() {
    let caps = MockBackend.capabilities();
    assert!(!caps.contains_key(&Capability::McpClient));
    assert!(!caps.contains_key(&Capability::McpServer));
}

#[tokio::test]
async fn t107_workspace_mode_staged_default() {
    let work_order = WorkOrderBuilder::new("staged test").build();
    assert!(matches!(work_order.workspace.mode, WorkspaceMode::Staged));
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend
        .run(Uuid::new_v4(), work_order, tx)
        .await
        .unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t108_include_exclude_globs_config() {
    let work_order = WorkOrderBuilder::new("glob test")
        .include(vec!["src/**/*.rs".into()])
        .exclude(vec!["target/**".into()])
        .build();
    assert_eq!(work_order.workspace.include, vec!["src/**/*.rs"]);
    assert_eq!(work_order.workspace.exclude, vec!["target/**"]);
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend
        .run(Uuid::new_v4(), work_order, tx)
        .await
        .unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}
