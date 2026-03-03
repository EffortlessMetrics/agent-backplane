// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]
//! Comprehensive deep tests for `abp-backend-mock` crate:
//! construction, configuration, event streaming, receipt generation,
//! sequential runs, concurrency, error simulation, Backend trait
//! compliance, WorkOrder variations, policy constraints, ordering
//! semantics, and receipt hash correctness.

use std::collections::BTreeMap;
use std::sync::Arc;

use abp_backend_mock::MockBackend;
use abp_core::{
    AgentEvent, AgentEventKind, CONTRACT_VERSION, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ContextPacket, ContextSnippet, ExecutionLane,
    ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt, RuntimeConfig, SupportLevel,
    WorkOrder, WorkOrderBuilder, WorkspaceMode, WorkspaceSpec, receipt_hash,
};
use abp_integrations::Backend;
use serde_json::json;
use tokio::sync::mpsc;
use uuid::Uuid;

// ===========================================================================
// Helpers
// ===========================================================================

fn wo(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task).build()
}

async fn run_collect(task: &str) -> (Receipt, Vec<AgentEvent>) {
    let (tx, mut rx) = mpsc::channel(64);
    let receipt = MockBackend.run(Uuid::new_v4(), wo(task), tx).await.unwrap();
    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    (receipt, events)
}

async fn run_receipt(task: &str) -> Receipt {
    let (tx, _rx) = mpsc::channel(64);
    MockBackend.run(Uuid::new_v4(), wo(task), tx).await.unwrap()
}

fn manual_wo(task: &str) -> WorkOrder {
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
// Section 1 – Construction and configuration (t01–t08)
// ===========================================================================

#[test]
fn t01_mock_backend_is_unit_struct() {
    let _b = MockBackend;
}

#[test]
fn t02_mock_backend_is_clone() {
    let a = MockBackend;
    let _b = a.clone();
}

#[test]
fn t03_mock_backend_is_debug() {
    let b = MockBackend;
    let dbg = format!("{b:?}");
    assert!(dbg.contains("MockBackend"));
}

#[test]
fn t04_mock_backend_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<MockBackend>();
}

#[test]
fn t05_mock_backend_is_sync() {
    fn assert_sync<T: Sync>() {}
    assert_sync::<MockBackend>();
}

#[test]
fn t06_mock_backend_identity_id() {
    assert_eq!(MockBackend.identity().id, "mock");
}

#[test]
fn t07_mock_backend_identity_versions() {
    let id = MockBackend.identity();
    assert_eq!(id.backend_version.as_deref(), Some("0.1"));
    assert_eq!(id.adapter_version.as_deref(), Some("0.1"));
}

#[test]
fn t08_identity_is_deterministic() {
    let a = MockBackend.identity();
    let b = MockBackend.identity();
    assert_eq!(a.id, b.id);
    assert_eq!(a.backend_version, b.backend_version);
    assert_eq!(a.adapter_version, b.adapter_version);
}

// ===========================================================================
// Section 2 – Capabilities (t09–t18)
// ===========================================================================

#[test]
fn t09_capabilities_contains_streaming() {
    let caps = MockBackend.capabilities();
    assert!(caps.contains_key(&Capability::Streaming));
}

#[test]
fn t10_streaming_is_native() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::Streaming),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn t11_tool_read_is_emulated() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::ToolRead),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn t12_tool_write_is_emulated() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::ToolWrite),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn t13_tool_edit_is_emulated() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::ToolEdit),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn t14_tool_bash_is_emulated() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::ToolBash),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn t15_structured_output_is_emulated() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::StructuredOutputJsonSchema),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn t16_capabilities_has_exactly_six_entries() {
    assert_eq!(MockBackend.capabilities().len(), 6);
}

#[test]
fn t17_capabilities_deterministic() {
    let a = MockBackend.capabilities();
    let b = MockBackend.capabilities();
    assert_eq!(a.len(), b.len());
    for (k, v) in &a {
        assert_eq!(format!("{v:?}"), format!("{:?}", b.get(k).unwrap()));
    }
}

#[test]
fn t18_unsupported_capability_absent() {
    let caps = MockBackend.capabilities();
    assert!(!caps.contains_key(&Capability::McpClient));
    assert!(!caps.contains_key(&Capability::McpServer));
    assert!(!caps.contains_key(&Capability::SessionResume));
}

// ===========================================================================
// Section 3 – Event streaming behavior (t19–t32)
// ===========================================================================

#[tokio::test]
async fn t19_produces_exactly_four_events() {
    let (_, events) = run_collect("hello").await;
    assert_eq!(events.len(), 4);
}

#[tokio::test]
async fn t20_first_event_is_run_started() {
    let (_, events) = run_collect("my task").await;
    assert!(matches!(&events[0].kind, AgentEventKind::RunStarted { .. }));
}

#[tokio::test]
async fn t21_run_started_message_contains_task() {
    let (_, events) = run_collect("my task").await;
    if let AgentEventKind::RunStarted { message } = &events[0].kind {
        assert!(message.contains("my task"));
    } else {
        panic!("expected RunStarted");
    }
}

#[tokio::test]
async fn t22_second_event_is_assistant_message() {
    let (_, events) = run_collect("x").await;
    assert!(matches!(
        &events[1].kind,
        AgentEventKind::AssistantMessage { .. }
    ));
}

#[tokio::test]
async fn t23_second_event_text_mentions_mock() {
    let (_, events) = run_collect("x").await;
    if let AgentEventKind::AssistantMessage { text } = &events[1].kind {
        assert!(text.contains("mock backend"));
    } else {
        panic!("expected AssistantMessage");
    }
}

#[tokio::test]
async fn t24_third_event_is_assistant_message() {
    let (_, events) = run_collect("x").await;
    assert!(matches!(
        &events[2].kind,
        AgentEventKind::AssistantMessage { .. }
    ));
}

#[tokio::test]
async fn t25_third_event_text_mentions_sidecar() {
    let (_, events) = run_collect("x").await;
    if let AgentEventKind::AssistantMessage { text } = &events[2].kind {
        assert!(text.contains("sidecar"));
    } else {
        panic!("expected AssistantMessage");
    }
}

#[tokio::test]
async fn t26_fourth_event_is_run_completed() {
    let (_, events) = run_collect("x").await;
    assert!(matches!(
        &events[3].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[tokio::test]
async fn t27_run_completed_message() {
    let (_, events) = run_collect("x").await;
    if let AgentEventKind::RunCompleted { message } = &events[3].kind {
        assert_eq!(message, "mock run complete");
    } else {
        panic!("expected RunCompleted");
    }
}

#[tokio::test]
async fn t28_all_events_have_ext_none() {
    let (_, events) = run_collect("x").await;
    for ev in &events {
        assert!(ev.ext.is_none());
    }
}

#[tokio::test]
async fn t29_event_timestamps_are_non_decreasing() {
    let (_, events) = run_collect("x").await;
    for w in events.windows(2) {
        assert!(w[1].ts >= w[0].ts);
    }
}

#[tokio::test]
async fn t30_events_arrive_in_order_via_channel() {
    let (tx, mut rx) = mpsc::channel(1);
    let handle = tokio::spawn(async move {
        let mut collected = Vec::new();
        while let Some(ev) = rx.recv().await {
            collected.push(ev);
        }
        collected
    });

    let _receipt = MockBackend
        .run(Uuid::new_v4(), wo("order"), tx)
        .await
        .unwrap();

    let events = handle.await.unwrap();
    assert_eq!(events.len(), 4);
    assert!(matches!(&events[0].kind, AgentEventKind::RunStarted { .. }));
    assert!(matches!(
        &events[3].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[tokio::test]
async fn t31_channel_capacity_one_still_works() {
    let (tx, mut rx) = mpsc::channel(1);
    let consumer = tokio::spawn(async move {
        let mut n = 0u32;
        while let Some(_ev) = rx.recv().await {
            n += 1;
        }
        n
    });
    let _receipt = MockBackend
        .run(Uuid::new_v4(), wo("cap1"), tx)
        .await
        .unwrap();
    let count = consumer.await.unwrap();
    assert_eq!(count, 4);
}

#[tokio::test]
async fn t32_dropped_receiver_does_not_panic() {
    let (tx, rx) = mpsc::channel(4);
    drop(rx);
    let result = MockBackend.run(Uuid::new_v4(), wo("drop"), tx).await;
    assert!(result.is_ok());
}

// ===========================================================================
// Section 4 – Receipt generation (t33–t48)
// ===========================================================================

#[tokio::test]
async fn t33_receipt_outcome_is_complete() {
    let r = run_receipt("receipt").await;
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t34_receipt_has_sha256() {
    let r = run_receipt("hash").await;
    assert!(r.receipt_sha256.is_some());
}

#[tokio::test]
async fn t35_receipt_sha256_is_64_hex_chars() {
    let r = run_receipt("hex").await;
    let hash = r.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[tokio::test]
async fn t36_receipt_contract_version() {
    let r = run_receipt("ver").await;
    assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn t37_receipt_backend_id_is_mock() {
    let r = run_receipt("id").await;
    assert_eq!(r.backend.id, "mock");
}

#[tokio::test]
async fn t38_receipt_usage_tokens_zero() {
    let r = run_receipt("usage").await;
    assert_eq!(r.usage.input_tokens, Some(0));
    assert_eq!(r.usage.output_tokens, Some(0));
}

#[tokio::test]
async fn t39_receipt_estimated_cost_zero() {
    let r = run_receipt("cost").await;
    assert_eq!(r.usage.estimated_cost_usd, Some(0.0));
}

#[tokio::test]
async fn t40_receipt_usage_raw_contains_note() {
    let r = run_receipt("raw").await;
    assert_eq!(r.usage_raw, json!({"note": "mock"}));
}

#[tokio::test]
async fn t41_receipt_artifacts_empty() {
    let r = run_receipt("art").await;
    assert!(r.artifacts.is_empty());
}

#[tokio::test]
async fn t42_receipt_verification_harness_ok() {
    let r = run_receipt("harness").await;
    assert!(r.verification.harness_ok);
    assert!(r.verification.git_diff.is_none());
    assert!(r.verification.git_status.is_none());
}

#[tokio::test]
async fn t43_receipt_mode_is_mapped_by_default() {
    let r = run_receipt("mode").await;
    assert_eq!(r.mode, ExecutionMode::Mapped);
}

#[tokio::test]
async fn t44_receipt_trace_has_four_entries() {
    let r = run_receipt("trace").await;
    assert_eq!(r.trace.len(), 4);
}

#[tokio::test]
async fn t45_receipt_trace_matches_streamed_events() {
    let (receipt, events) = run_collect("match").await;
    assert_eq!(receipt.trace.len(), events.len());
    for (t, e) in receipt.trace.iter().zip(events.iter()) {
        assert_eq!(format!("{:?}", t.kind), format!("{:?}", e.kind));
    }
}

#[tokio::test]
async fn t46_receipt_run_id_matches_supplied() {
    let run_id = Uuid::new_v4();
    let (tx, _rx) = mpsc::channel(16);
    let r = MockBackend.run(run_id, wo("runid"), tx).await.unwrap();
    assert_eq!(r.meta.run_id, run_id);
}

#[tokio::test]
async fn t47_receipt_work_order_id_propagated() {
    let order = wo("prop");
    let wo_id = order.id;
    let (tx, _rx) = mpsc::channel(16);
    let r = MockBackend.run(Uuid::new_v4(), order, tx).await.unwrap();
    assert_eq!(r.meta.work_order_id, wo_id);
}

#[tokio::test]
async fn t48_receipt_finished_after_started() {
    let r = run_receipt("timing").await;
    assert!(r.meta.finished_at >= r.meta.started_at);
}

// ===========================================================================
// Section 5 – Receipt hash correctness (t49–t56)
// ===========================================================================

#[tokio::test]
async fn t49_hash_deterministic_for_same_receipt_content() {
    let r = run_receipt("det").await;
    let recomputed = receipt_hash(&r).unwrap();
    assert_eq!(r.receipt_sha256.as_ref().unwrap(), &recomputed);
}

#[tokio::test]
async fn t50_hash_excludes_receipt_sha256_field() {
    let r = run_receipt("excl").await;
    let mut r2 = r.clone();
    r2.receipt_sha256 = None;
    let h2 = receipt_hash(&r2).unwrap();
    assert_eq!(r.receipt_sha256.as_ref().unwrap(), &h2);
}

#[tokio::test]
async fn t51_different_tasks_produce_different_hashes() {
    let r1 = run_receipt("alpha").await;
    let r2 = run_receipt("beta").await;
    assert_ne!(r1.receipt_sha256, r2.receipt_sha256);
}

#[tokio::test]
async fn t52_hash_changes_with_different_run_id() {
    let (tx1, _) = mpsc::channel(16);
    let (tx2, _) = mpsc::channel(16);
    let r1 = MockBackend
        .run(Uuid::from_u128(1), wo("same"), tx1)
        .await
        .unwrap();
    let r2 = MockBackend
        .run(Uuid::from_u128(2), wo("same"), tx2)
        .await
        .unwrap();
    assert_ne!(r1.receipt_sha256, r2.receipt_sha256);
}

#[tokio::test]
async fn t53_receipt_serializes_to_json() {
    let r = run_receipt("json").await;
    let json_str = serde_json::to_string(&r).unwrap();
    assert!(json_str.contains("mock"));
}

#[tokio::test]
async fn t54_receipt_roundtrips_through_json() {
    let r = run_receipt("round").await;
    let json_str = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json_str).unwrap();
    assert_eq!(r.receipt_sha256, r2.receipt_sha256);
    assert_eq!(r.outcome, r2.outcome);
}

#[tokio::test]
async fn t55_receipt_with_hash_called_twice_stable() {
    let (tx, _rx) = mpsc::channel(16);
    let r = MockBackend
        .run(Uuid::new_v4(), wo("twice"), tx)
        .await
        .unwrap();
    let h1 = r.receipt_sha256.clone();
    let r2 = Receipt {
        receipt_sha256: None,
        ..r
    }
    .with_hash()
    .unwrap();
    assert_eq!(h1, r2.receipt_sha256);
}

#[tokio::test]
async fn t56_receipt_hash_is_sha256_hex() {
    let r = run_receipt("sha").await;
    let h = r.receipt_sha256.unwrap();
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')));
}

// ===========================================================================
// Section 6 – Multiple sequential runs (t57–t63)
// ===========================================================================

#[tokio::test]
async fn t57_two_sequential_runs_both_succeed() {
    let r1 = run_receipt("first").await;
    let r2 = run_receipt("second").await;
    assert_eq!(r1.outcome, Outcome::Complete);
    assert_eq!(r2.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t58_sequential_runs_have_unique_hashes() {
    let r1 = run_receipt("seq1").await;
    let r2 = run_receipt("seq2").await;
    assert_ne!(r1.receipt_sha256, r2.receipt_sha256);
}

#[tokio::test]
async fn t59_ten_sequential_runs() {
    for i in 0..10 {
        let r = run_receipt(&format!("iter-{i}")).await;
        assert_eq!(r.outcome, Outcome::Complete);
        assert!(r.receipt_sha256.is_some());
    }
}

#[tokio::test]
async fn t60_sequential_runs_propagate_distinct_run_ids() {
    let mut run_ids = Vec::new();
    for _ in 0..5 {
        let id = Uuid::new_v4();
        let (tx, _) = mpsc::channel(16);
        let r = MockBackend.run(id, wo("multi"), tx).await.unwrap();
        assert_eq!(r.meta.run_id, id);
        run_ids.push(id);
    }
    let unique: std::collections::HashSet<_> = run_ids.iter().collect();
    assert_eq!(unique.len(), 5);
}

#[tokio::test]
async fn t61_sequential_runs_each_produce_four_events() {
    for _ in 0..3 {
        let (_, events) = run_collect("count").await;
        assert_eq!(events.len(), 4);
    }
}

#[tokio::test]
async fn t62_same_work_order_reused_across_runs() {
    let order = wo("reuse");
    let wo_id = order.id;
    for _ in 0..3 {
        let (tx, _) = mpsc::channel(16);
        let r = MockBackend
            .run(Uuid::new_v4(), order.clone(), tx)
            .await
            .unwrap();
        assert_eq!(r.meta.work_order_id, wo_id);
    }
}

#[tokio::test]
async fn t63_sequential_runs_finished_at_non_decreasing() {
    let mut prev = None;
    for i in 0..3 {
        let r = run_receipt(&format!("ts-{i}")).await;
        if let Some(p) = prev {
            assert!(r.meta.started_at >= p);
        }
        prev = Some(r.meta.finished_at);
    }
}

// ===========================================================================
// Section 7 – Concurrent mock backend usage (t64–t72)
// ===========================================================================

#[tokio::test]
async fn t64_two_concurrent_runs() {
    let (tx1, _) = mpsc::channel(16);
    let (tx2, _) = mpsc::channel(16);
    let b = Arc::new(MockBackend);
    let b2 = b.clone();
    let h1 = tokio::spawn(async move { b.run(Uuid::new_v4(), wo("c1"), tx1).await.unwrap() });
    let h2 = tokio::spawn(async move { b2.run(Uuid::new_v4(), wo("c2"), tx2).await.unwrap() });
    let (r1, r2) = tokio::join!(h1, h2);
    assert_eq!(r1.unwrap().outcome, Outcome::Complete);
    assert_eq!(r2.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn t65_ten_concurrent_runs() {
    let b = Arc::new(MockBackend);
    let mut handles = Vec::new();
    for i in 0..10 {
        let b = b.clone();
        handles.push(tokio::spawn(async move {
            let (tx, _) = mpsc::channel(16);
            b.run(Uuid::new_v4(), wo(&format!("par-{i}")), tx)
                .await
                .unwrap()
        }));
    }
    let results: Vec<_> = futures::future::join_all(handles)
        .await
        .into_iter()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(results.len(), 10);
    assert!(results.iter().all(|r| r.outcome == Outcome::Complete));
}

#[tokio::test]
async fn t66_concurrent_runs_produce_unique_hashes() {
    let b = Arc::new(MockBackend);
    let mut handles = Vec::new();
    for i in 0..5 {
        let b = b.clone();
        handles.push(tokio::spawn(async move {
            let (tx, _) = mpsc::channel(16);
            b.run(Uuid::new_v4(), wo(&format!("uh-{i}")), tx)
                .await
                .unwrap()
        }));
    }
    let results: Vec<_> = futures::future::join_all(handles)
        .await
        .into_iter()
        .map(|r| r.unwrap())
        .collect();
    let hashes: std::collections::HashSet<_> = results
        .iter()
        .map(|r| r.receipt_sha256.clone().unwrap())
        .collect();
    assert_eq!(hashes.len(), 5);
}

#[tokio::test]
async fn t67_concurrent_event_counts_correct() {
    let b = Arc::new(MockBackend);
    let mut handles = Vec::new();
    for i in 0..5 {
        let b = b.clone();
        handles.push(tokio::spawn(async move {
            let (tx, mut rx) = mpsc::channel(16);
            let _r = b
                .run(Uuid::new_v4(), wo(&format!("ev-{i}")), tx)
                .await
                .unwrap();
            let mut count = 0u32;
            while rx.try_recv().is_ok() {
                count += 1;
            }
            count
        }));
    }
    let counts: Vec<_> = futures::future::join_all(handles)
        .await
        .into_iter()
        .map(|r| r.unwrap())
        .collect();
    assert!(counts.iter().all(|&c| c == 4));
}

#[tokio::test]
async fn t68_backend_behind_arc_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Arc<MockBackend>>();
}

#[tokio::test]
async fn t69_boxed_dyn_backend_works() {
    let b: Box<dyn Backend> = Box::new(MockBackend);
    let (tx, _) = mpsc::channel(16);
    let r = b.run(Uuid::new_v4(), wo("boxed"), tx).await.unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t70_arc_dyn_backend_concurrent() {
    let b: Arc<dyn Backend> = Arc::new(MockBackend);
    let b2 = b.clone();
    let (tx1, _) = mpsc::channel(16);
    let (tx2, _) = mpsc::channel(16);
    let h1 = tokio::spawn(async move { b.run(Uuid::new_v4(), wo("arc1"), tx1).await.unwrap() });
    let h2 = tokio::spawn(async move { b2.run(Uuid::new_v4(), wo("arc2"), tx2).await.unwrap() });
    let (r1, r2) = tokio::join!(h1, h2);
    assert_eq!(r1.unwrap().outcome, Outcome::Complete);
    assert_eq!(r2.unwrap().outcome, Outcome::Complete);
}

#[tokio::test]
async fn t71_concurrent_runs_all_have_contract_version() {
    let b = Arc::new(MockBackend);
    let mut handles = Vec::new();
    for i in 0..4 {
        let b = b.clone();
        handles.push(tokio::spawn(async move {
            let (tx, _) = mpsc::channel(16);
            b.run(Uuid::new_v4(), wo(&format!("cv-{i}")), tx)
                .await
                .unwrap()
        }));
    }
    for h in handles {
        let r = h.await.unwrap();
        assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
    }
}

#[tokio::test]
async fn t72_concurrent_consumers_receive_all_events() {
    let b = Arc::new(MockBackend);
    let mut handles = Vec::new();
    for i in 0..4 {
        let b = b.clone();
        handles.push(tokio::spawn(async move {
            let (tx, mut rx) = mpsc::channel(1);
            let consumer = tokio::spawn(async move {
                let mut evs = Vec::new();
                while let Some(ev) = rx.recv().await {
                    evs.push(ev);
                }
                evs
            });
            let _r = b
                .run(Uuid::new_v4(), wo(&format!("cc-{i}")), tx)
                .await
                .unwrap();
            consumer.await.unwrap()
        }));
    }
    for h in handles {
        let events = h.await.unwrap();
        assert_eq!(events.len(), 4);
    }
}

// ===========================================================================
// Section 8 – Backend trait compliance (t73–t80)
// ===========================================================================

#[test]
fn t73_backend_trait_is_object_safe() {
    fn _accept(_: &dyn Backend) {}
    _accept(&MockBackend);
}

#[tokio::test]
async fn t74_backend_identity_returns_backend_identity() {
    let id = MockBackend.identity();
    assert!(!id.id.is_empty());
}

#[tokio::test]
async fn t75_backend_capabilities_returns_manifest() {
    let caps: CapabilityManifest = MockBackend.capabilities();
    assert!(!caps.is_empty());
}

#[tokio::test]
async fn t76_backend_run_returns_result_receipt() {
    let (tx, _) = mpsc::channel(16);
    let result: anyhow::Result<Receipt> = MockBackend.run(Uuid::new_v4(), wo("trait"), tx).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn t77_backend_in_hashmap() {
    let mut map: BTreeMap<String, Box<dyn Backend>> = BTreeMap::new();
    map.insert("mock".into(), Box::new(MockBackend));
    let b = map.get("mock").unwrap();
    assert_eq!(b.identity().id, "mock");
}

#[tokio::test]
async fn t78_backend_vec_dispatch() {
    let backends: Vec<Box<dyn Backend>> = vec![Box::new(MockBackend), Box::new(MockBackend)];
    for b in &backends {
        let (tx, _) = mpsc::channel(16);
        let r = b.run(Uuid::new_v4(), wo("vec"), tx).await.unwrap();
        assert_eq!(r.outcome, Outcome::Complete);
    }
}

#[tokio::test]
async fn t79_cloned_backend_runs_independently() {
    let a = MockBackend;
    let b = a.clone();
    let (tx1, _) = mpsc::channel(16);
    let (tx2, _) = mpsc::channel(16);
    let r1 = a.run(Uuid::new_v4(), wo("a"), tx1).await.unwrap();
    let r2 = b.run(Uuid::new_v4(), wo("b"), tx2).await.unwrap();
    assert_eq!(r1.outcome, Outcome::Complete);
    assert_eq!(r2.outcome, Outcome::Complete);
}

#[test]
fn t80_backend_trait_requires_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MockBackend>();
}

// ===========================================================================
// Section 9 – WorkOrder configurations (t81–t93)
// ===========================================================================

#[tokio::test]
async fn t81_empty_task_string() {
    let r = run_receipt("").await;
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t82_very_long_task_string() {
    let task = "z".repeat(100_000);
    let r = run_receipt(&task).await;
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t83_workspace_first_lane() {
    let order = WorkOrderBuilder::new("ws-first")
        .lane(ExecutionLane::WorkspaceFirst)
        .build();
    let (tx, _) = mpsc::channel(16);
    let r = MockBackend.run(Uuid::new_v4(), order, tx).await.unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t84_patch_first_lane() {
    let order = WorkOrderBuilder::new("patch-first")
        .lane(ExecutionLane::PatchFirst)
        .build();
    let (tx, _) = mpsc::channel(16);
    let r = MockBackend.run(Uuid::new_v4(), order, tx).await.unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t85_with_model_config() {
    let order = WorkOrderBuilder::new("model").model("gpt-4").build();
    let (tx, _) = mpsc::channel(16);
    let r = MockBackend.run(Uuid::new_v4(), order, tx).await.unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t86_with_max_budget() {
    let order = WorkOrderBuilder::new("budget").max_budget_usd(1.50).build();
    let (tx, _) = mpsc::channel(16);
    let r = MockBackend.run(Uuid::new_v4(), order, tx).await.unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t87_with_max_turns() {
    let order = WorkOrderBuilder::new("turns").max_turns(5).build();
    let (tx, _) = mpsc::channel(16);
    let r = MockBackend.run(Uuid::new_v4(), order, tx).await.unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t88_with_context_files() {
    let ctx = ContextPacket {
        files: vec!["src/main.rs".into(), "Cargo.toml".into()],
        snippets: vec![],
    };
    let order = WorkOrderBuilder::new("ctx").context(ctx).build();
    let (tx, _) = mpsc::channel(16);
    let r = MockBackend.run(Uuid::new_v4(), order, tx).await.unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t89_with_context_snippets() {
    let ctx = ContextPacket {
        files: vec![],
        snippets: vec![ContextSnippet {
            name: "hint".into(),
            content: "use async".into(),
        }],
    };
    let order = WorkOrderBuilder::new("snip").context(ctx).build();
    let (tx, _) = mpsc::channel(16);
    let r = MockBackend.run(Uuid::new_v4(), order, tx).await.unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t90_with_include_exclude_globs() {
    let order = WorkOrderBuilder::new("globs")
        .include(vec!["**/*.rs".into()])
        .exclude(vec!["target/**".into()])
        .build();
    let (tx, _) = mpsc::channel(16);
    let r = MockBackend.run(Uuid::new_v4(), order, tx).await.unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t91_with_env_vars_in_config() {
    let mut config = RuntimeConfig::default();
    config.env.insert("MY_VAR".into(), "hello".into());
    let order = WorkOrderBuilder::new("env").config(config).build();
    let (tx, _) = mpsc::channel(16);
    let r = MockBackend.run(Uuid::new_v4(), order, tx).await.unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t92_with_vendor_flags() {
    let mut config = RuntimeConfig::default();
    config
        .vendor
        .insert("custom".into(), json!({"key": "value"}));
    let order = WorkOrderBuilder::new("vendor").config(config).build();
    let (tx, _) = mpsc::channel(16);
    let r = MockBackend.run(Uuid::new_v4(), order, tx).await.unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t93_nil_uuid_work_order_id() {
    let order = manual_wo("nil");
    let (tx, _) = mpsc::channel(16);
    let r = MockBackend.run(Uuid::new_v4(), order, tx).await.unwrap();
    assert_eq!(r.meta.work_order_id, Uuid::nil());
}

// ===========================================================================
// Section 10 – Execution mode variations (t94–t99)
// ===========================================================================

#[tokio::test]
async fn t94_default_mode_is_mapped() {
    let r = run_receipt("mapped").await;
    assert_eq!(r.mode, ExecutionMode::Mapped);
}

#[tokio::test]
async fn t95_passthrough_mode_via_vendor_config() {
    let mut config = RuntimeConfig::default();
    config
        .vendor
        .insert("abp".into(), json!({"mode": "passthrough"}));
    let order = WorkOrderBuilder::new("pt").config(config).build();
    let (tx, _) = mpsc::channel(16);
    let r = MockBackend.run(Uuid::new_v4(), order, tx).await.unwrap();
    assert_eq!(r.mode, ExecutionMode::Passthrough);
}

#[tokio::test]
async fn t96_mapped_mode_via_vendor_config() {
    let mut config = RuntimeConfig::default();
    config
        .vendor
        .insert("abp".into(), json!({"mode": "mapped"}));
    let order = WorkOrderBuilder::new("mp").config(config).build();
    let (tx, _) = mpsc::channel(16);
    let r = MockBackend.run(Uuid::new_v4(), order, tx).await.unwrap();
    assert_eq!(r.mode, ExecutionMode::Mapped);
}

#[tokio::test]
async fn t97_passthrough_via_dotted_key() {
    let mut config = RuntimeConfig::default();
    config
        .vendor
        .insert("abp.mode".into(), json!("passthrough"));
    let order = WorkOrderBuilder::new("dot").config(config).build();
    let (tx, _) = mpsc::channel(16);
    let r = MockBackend.run(Uuid::new_v4(), order, tx).await.unwrap();
    assert_eq!(r.mode, ExecutionMode::Passthrough);
}

#[tokio::test]
async fn t98_invalid_mode_falls_back_to_mapped() {
    let mut config = RuntimeConfig::default();
    config
        .vendor
        .insert("abp".into(), json!({"mode": "nonexistent"}));
    let order = WorkOrderBuilder::new("inv").config(config).build();
    let (tx, _) = mpsc::channel(16);
    let r = MockBackend.run(Uuid::new_v4(), order, tx).await.unwrap();
    assert_eq!(r.mode, ExecutionMode::Mapped);
}

#[tokio::test]
async fn t99_no_abp_vendor_key_defaults_mapped() {
    let mut config = RuntimeConfig::default();
    config.vendor.insert("other".into(), json!("foo"));
    let order = WorkOrderBuilder::new("no-abp").config(config).build();
    let (tx, _) = mpsc::channel(16);
    let r = MockBackend.run(Uuid::new_v4(), order, tx).await.unwrap();
    assert_eq!(r.mode, ExecutionMode::Mapped);
}

// ===========================================================================
// Section 11 – Policy constraints (t100–t106)
// ===========================================================================

#[tokio::test]
async fn t100_empty_policy_succeeds() {
    let order = WorkOrderBuilder::new("nopol")
        .policy(PolicyProfile::default())
        .build();
    let (tx, _) = mpsc::channel(16);
    let r = MockBackend.run(Uuid::new_v4(), order, tx).await.unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t101_policy_with_allowed_tools() {
    let policy = PolicyProfile {
        allowed_tools: vec!["read".into(), "write".into()],
        ..Default::default()
    };
    let order = WorkOrderBuilder::new("allow").policy(policy).build();
    let (tx, _) = mpsc::channel(16);
    let r = MockBackend.run(Uuid::new_v4(), order, tx).await.unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t102_policy_with_disallowed_tools() {
    let policy = PolicyProfile {
        disallowed_tools: vec!["bash".into()],
        ..Default::default()
    };
    let order = WorkOrderBuilder::new("deny").policy(policy).build();
    let (tx, _) = mpsc::channel(16);
    let r = MockBackend.run(Uuid::new_v4(), order, tx).await.unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t103_policy_with_deny_read_globs() {
    let policy = PolicyProfile {
        deny_read: vec!["/etc/**".into()],
        ..Default::default()
    };
    let order = WorkOrderBuilder::new("deny-read").policy(policy).build();
    let (tx, _) = mpsc::channel(16);
    let r = MockBackend.run(Uuid::new_v4(), order, tx).await.unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t104_policy_with_deny_write_globs() {
    let policy = PolicyProfile {
        deny_write: vec!["*.lock".into()],
        ..Default::default()
    };
    let order = WorkOrderBuilder::new("deny-write").policy(policy).build();
    let (tx, _) = mpsc::channel(16);
    let r = MockBackend.run(Uuid::new_v4(), order, tx).await.unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t105_policy_with_network_rules() {
    let policy = PolicyProfile {
        allow_network: vec!["api.example.com".into()],
        deny_network: vec!["evil.com".into()],
        ..Default::default()
    };
    let order = WorkOrderBuilder::new("net").policy(policy).build();
    let (tx, _) = mpsc::channel(16);
    let r = MockBackend.run(Uuid::new_v4(), order, tx).await.unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t106_policy_with_approval_required() {
    let policy = PolicyProfile {
        require_approval_for: vec!["bash".into(), "write".into()],
        ..Default::default()
    };
    let order = WorkOrderBuilder::new("approval").policy(policy).build();
    let (tx, _) = mpsc::channel(16);
    let r = MockBackend.run(Uuid::new_v4(), order, tx).await.unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
}

// ===========================================================================
// Section 12 – Capability requirements (error simulation) (t107–t116)
// ===========================================================================

#[tokio::test]
async fn t107_satisfied_emulated_requirement() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Emulated,
        }],
    };
    let order = WorkOrderBuilder::new("ok-emu").requirements(reqs).build();
    let (tx, _) = mpsc::channel(16);
    let r = MockBackend.run(Uuid::new_v4(), order, tx).await;
    assert!(r.is_ok());
}

#[tokio::test]
async fn t108_satisfied_native_streaming() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    let order = WorkOrderBuilder::new("ok-nat").requirements(reqs).build();
    let (tx, _) = mpsc::channel(16);
    assert!(MockBackend.run(Uuid::new_v4(), order, tx).await.is_ok());
}

#[tokio::test]
async fn t109_unsatisfied_native_requirement_on_emulated() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Native,
        }],
    };
    let order = WorkOrderBuilder::new("fail-nat").requirements(reqs).build();
    let (tx, _) = mpsc::channel(16);
    let result = MockBackend.run(Uuid::new_v4(), order, tx).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn t110_unsatisfied_missing_capability() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Emulated,
        }],
    };
    let order = WorkOrderBuilder::new("fail-miss")
        .requirements(reqs)
        .build();
    let (tx, _) = mpsc::channel(16);
    assert!(MockBackend.run(Uuid::new_v4(), order, tx).await.is_err());
}

#[tokio::test]
async fn t111_error_message_contains_capability_name() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpServer,
            min_support: MinSupport::Emulated,
        }],
    };
    let order = WorkOrderBuilder::new("err-msg").requirements(reqs).build();
    let (tx, _) = mpsc::channel(16);
    let err = MockBackend
        .run(Uuid::new_v4(), order, tx)
        .await
        .unwrap_err();
    let msg = format!("{err:?}");
    assert!(msg.contains("McpServer") || msg.contains("mcp_server"));
}

#[tokio::test]
async fn t112_multiple_requirements_all_satisfied() {
    let reqs = CapabilityRequirements {
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
    let order = WorkOrderBuilder::new("multi-ok").requirements(reqs).build();
    let (tx, _) = mpsc::channel(16);
    assert!(MockBackend.run(Uuid::new_v4(), order, tx).await.is_ok());
}

#[tokio::test]
async fn t113_multiple_requirements_one_unsatisfied() {
    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::SessionResume,
                min_support: MinSupport::Emulated,
            },
        ],
    };
    let order = WorkOrderBuilder::new("multi-fail")
        .requirements(reqs)
        .build();
    let (tx, _) = mpsc::channel(16);
    assert!(MockBackend.run(Uuid::new_v4(), order, tx).await.is_err());
}

#[tokio::test]
async fn t114_empty_requirements_satisfied() {
    let reqs = CapabilityRequirements { required: vec![] };
    let order = WorkOrderBuilder::new("empty-req")
        .requirements(reqs)
        .build();
    let (tx, _) = mpsc::channel(16);
    assert!(MockBackend.run(Uuid::new_v4(), order, tx).await.is_ok());
}

#[tokio::test]
async fn t115_native_requirement_on_tool_write_emulated_fails() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolWrite,
            min_support: MinSupport::Native,
        }],
    };
    let order = WorkOrderBuilder::new("tw-nat").requirements(reqs).build();
    let (tx, _) = mpsc::channel(16);
    assert!(MockBackend.run(Uuid::new_v4(), order, tx).await.is_err());
}

#[tokio::test]
async fn t116_native_requirement_on_tool_bash_emulated_fails() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolBash,
            min_support: MinSupport::Native,
        }],
    };
    let order = WorkOrderBuilder::new("tb-nat").requirements(reqs).build();
    let (tx, _) = mpsc::channel(16);
    assert!(MockBackend.run(Uuid::new_v4(), order, tx).await.is_err());
}

// ===========================================================================
// Section 13 – Streaming semantics (t117–t124)
// ===========================================================================

#[tokio::test]
async fn t117_trace_timestamps_monotonic() {
    let r = run_receipt("mono").await;
    for w in r.trace.windows(2) {
        assert!(w[1].ts >= w[0].ts);
    }
}

#[tokio::test]
async fn t118_trace_first_event_run_started() {
    let r = run_receipt("first-ev").await;
    assert!(matches!(
        &r.trace[0].kind,
        AgentEventKind::RunStarted { .. }
    ));
}

#[tokio::test]
async fn t119_trace_last_event_run_completed() {
    let r = run_receipt("last-ev").await;
    assert!(matches!(
        &r.trace[3].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[tokio::test]
async fn t120_trace_middle_events_are_assistant_messages() {
    let r = run_receipt("mid").await;
    assert!(matches!(
        &r.trace[1].kind,
        AgentEventKind::AssistantMessage { .. }
    ));
    assert!(matches!(
        &r.trace[2].kind,
        AgentEventKind::AssistantMessage { .. }
    ));
}

#[tokio::test]
async fn t121_async_consumer_sees_all_events_before_receipt() {
    let (tx, mut rx) = mpsc::channel(64);
    let consumer = tokio::spawn(async move {
        let mut events = Vec::new();
        while let Some(ev) = rx.recv().await {
            events.push(ev);
        }
        events
    });
    let receipt = MockBackend
        .run(Uuid::new_v4(), wo("async-con"), tx)
        .await
        .unwrap();
    let events = consumer.await.unwrap();
    assert_eq!(events.len(), receipt.trace.len());
}

#[tokio::test]
async fn t122_large_channel_no_backpressure() {
    let (tx, mut rx) = mpsc::channel(1024);
    let _r = MockBackend
        .run(Uuid::new_v4(), wo("large-ch"), tx)
        .await
        .unwrap();
    let mut count = 0;
    while rx.try_recv().is_ok() {
        count += 1;
    }
    assert_eq!(count, 4);
}

#[tokio::test]
async fn t123_event_kind_names_in_trace() {
    let r = run_receipt("kinds").await;
    let kinds: Vec<String> = r
        .trace
        .iter()
        .map(|ev| {
            format!("{:?}", ev.kind)
                .split_whitespace()
                .next()
                .unwrap()
                .to_string()
        })
        .collect();
    assert!(kinds[0].contains("RunStarted"));
    assert!(kinds[1].contains("AssistantMessage"));
    assert!(kinds[2].contains("AssistantMessage"));
    assert!(kinds[3].contains("RunCompleted"));
}

#[tokio::test]
async fn t124_all_trace_events_have_ext_none() {
    let r = run_receipt("ext-none").await;
    for ev in &r.trace {
        assert!(ev.ext.is_none());
    }
}

// ===========================================================================
// Section 14 – Edge cases and special inputs (t125–t138)
// ===========================================================================

#[tokio::test]
async fn t125_unicode_task_string() {
    let r = run_receipt("こんにちは世界 🌍").await;
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t126_unicode_in_run_started_message() {
    let (_, events) = run_collect("日本語テスト").await;
    if let AgentEventKind::RunStarted { message } = &events[0].kind {
        assert!(message.contains("日本語テスト"));
    } else {
        panic!("expected RunStarted");
    }
}

#[tokio::test]
async fn t127_special_characters_in_task() {
    let r = run_receipt(r#"task with "quotes" and <brackets> & ampersand"#).await;
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t128_newlines_in_task() {
    let r = run_receipt("line1\nline2\nline3").await;
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t129_nil_uuid_run_id() {
    let (tx, _) = mpsc::channel(16);
    let r = MockBackend
        .run(Uuid::nil(), wo("nil-run"), tx)
        .await
        .unwrap();
    assert_eq!(r.meta.run_id, Uuid::nil());
}

#[tokio::test]
async fn t130_max_uuid_run_id() {
    let (tx, _) = mpsc::channel(16);
    let r = MockBackend
        .run(Uuid::max(), wo("max-run"), tx)
        .await
        .unwrap();
    assert_eq!(r.meta.run_id, Uuid::max());
}

#[tokio::test]
async fn t131_whitespace_only_task() {
    let r = run_receipt("   \t\n  ").await;
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t132_run_started_message_format() {
    let (_, events) = run_collect("fix bug").await;
    if let AgentEventKind::RunStarted { message } = &events[0].kind {
        assert_eq!(message, "mock backend starting: fix bug");
    } else {
        panic!("expected RunStarted");
    }
}

#[tokio::test]
async fn t133_duration_ms_is_reasonable() {
    let r = run_receipt("dur").await;
    // Mock backend should complete in well under a second
    assert!(r.meta.duration_ms < 5000);
}

#[tokio::test]
async fn t134_receipt_capabilities_match_backend_capabilities() {
    let r = run_receipt("cap-match").await;
    let backend_caps = MockBackend.capabilities();
    assert_eq!(r.capabilities.len(), backend_caps.len());
}

#[tokio::test]
async fn t135_receipt_backend_matches_identity() {
    let r = run_receipt("id-match").await;
    let identity = MockBackend.identity();
    assert_eq!(r.backend.id, identity.id);
    assert_eq!(r.backend.backend_version, identity.backend_version);
    assert_eq!(r.backend.adapter_version, identity.adapter_version);
}

#[tokio::test]
async fn t136_receipt_serde_roundtrip_preserves_trace() {
    let r = run_receipt("serde").await;
    let json = serde_json::to_string_pretty(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(r2.trace.len(), 4);
    assert_eq!(r2.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t137_receipt_serde_roundtrip_preserves_usage() {
    let r = run_receipt("usage-rt").await;
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(r2.usage.input_tokens, Some(0));
    assert_eq!(r2.usage.output_tokens, Some(0));
    assert_eq!(r2.usage.estimated_cost_usd, Some(0.0));
}

#[tokio::test]
async fn t138_receipt_serde_roundtrip_preserves_hash() {
    let r = run_receipt("hash-rt").await;
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(r.receipt_sha256, r2.receipt_sha256);
}

// ===========================================================================
// Section 15 – Workspace mode variations (t139–t143)
// ===========================================================================

#[tokio::test]
async fn t139_passthrough_workspace_mode() {
    let order = WorkOrderBuilder::new("ws-pt")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let (tx, _) = mpsc::channel(16);
    let r = MockBackend.run(Uuid::new_v4(), order, tx).await.unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t140_staged_workspace_mode() {
    let order = WorkOrderBuilder::new("ws-staged")
        .workspace_mode(WorkspaceMode::Staged)
        .build();
    let (tx, _) = mpsc::channel(16);
    let r = MockBackend.run(Uuid::new_v4(), order, tx).await.unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t141_custom_root() {
    let order = WorkOrderBuilder::new("custom-root")
        .root("/tmp/test-workspace")
        .build();
    let (tx, _) = mpsc::channel(16);
    let r = MockBackend.run(Uuid::new_v4(), order, tx).await.unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t142_empty_root() {
    let order = WorkOrderBuilder::new("empty-root").root("").build();
    let (tx, _) = mpsc::channel(16);
    let r = MockBackend.run(Uuid::new_v4(), order, tx).await.unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t143_complex_include_exclude() {
    let order = WorkOrderBuilder::new("complex-globs")
        .include(vec!["src/**/*.rs".into(), "tests/**/*.rs".into()])
        .exclude(vec![
            "target/**".into(),
            "**/*.lock".into(),
            ".git/**".into(),
        ])
        .build();
    let (tx, _) = mpsc::channel(16);
    let r = MockBackend.run(Uuid::new_v4(), order, tx).await.unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
}

// ===========================================================================
// Section 16 – Usage and verification fields (t144–t149)
// ===========================================================================

#[tokio::test]
async fn t144_usage_cache_tokens_none() {
    let r = run_receipt("cache").await;
    assert!(r.usage.cache_read_tokens.is_none());
    assert!(r.usage.cache_write_tokens.is_none());
}

#[tokio::test]
async fn t145_usage_request_units_none() {
    let r = run_receipt("units").await;
    assert!(r.usage.request_units.is_none());
}

#[tokio::test]
async fn t146_verification_report_defaults() {
    let r = run_receipt("verify").await;
    assert!(r.verification.git_diff.is_none());
    assert!(r.verification.git_status.is_none());
    assert!(r.verification.harness_ok);
}

#[tokio::test]
async fn t147_receipt_json_includes_all_fields() {
    let r = run_receipt("fields").await;
    let v: serde_json::Value = serde_json::to_value(&r).unwrap();
    assert!(v.get("meta").is_some());
    assert!(v.get("backend").is_some());
    assert!(v.get("capabilities").is_some());
    assert!(v.get("mode").is_some());
    assert!(v.get("usage_raw").is_some());
    assert!(v.get("usage").is_some());
    assert!(v.get("trace").is_some());
    assert!(v.get("artifacts").is_some());
    assert!(v.get("verification").is_some());
    assert!(v.get("outcome").is_some());
    assert!(v.get("receipt_sha256").is_some());
}

#[tokio::test]
async fn t148_receipt_meta_json_fields() {
    let r = run_receipt("meta-fields").await;
    let v: serde_json::Value = serde_json::to_value(&r.meta).unwrap();
    assert!(v.get("run_id").is_some());
    assert!(v.get("work_order_id").is_some());
    assert!(v.get("contract_version").is_some());
    assert!(v.get("started_at").is_some());
    assert!(v.get("finished_at").is_some());
    assert!(v.get("duration_ms").is_some());
}

#[tokio::test]
async fn t149_outcome_serializes_as_string() {
    let r = run_receipt("outcome-ser").await;
    let v: serde_json::Value = serde_json::to_value(&r.outcome).unwrap();
    assert_eq!(v.as_str(), Some("complete"));
}
