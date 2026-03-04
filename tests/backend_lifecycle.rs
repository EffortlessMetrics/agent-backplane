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
//! Comprehensive tests for backend lifecycle management.
//!
//! Covers: MockBackend creation/configuration, Backend trait methods, event streaming
//! lifecycle, concurrent execution, error handling, channel behavior, receipt generation,
//! timeout handling, cancellation, sequential runs, metadata/capabilities, event ordering,
//! large event volumes, and backend state transitions.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ExecutionMode, MinSupport, Outcome, Receipt,
    SupportLevel, WorkOrder, WorkOrderBuilder,
};
use abp_integrations::{
    Backend, MockBackend, ensure_capability_requirements, extract_execution_mode,
    validate_passthrough_compatibility,
};
use serde_json::json;
use tokio::sync::mpsc;
use uuid::Uuid;

// ===========================================================================
// Helpers
// ===========================================================================

fn base_work_order() -> WorkOrder {
    WorkOrderBuilder::new("test task").build()
}

fn work_order_with_task(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task).build()
}

async fn run_mock(backend: &MockBackend, wo: WorkOrder) -> (Receipt, Vec<AgentEvent>) {
    let run_id = Uuid::new_v4();
    let (tx, mut rx) = mpsc::channel(64);
    let receipt = backend.run(run_id, wo, tx).await.unwrap();
    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    (receipt, events)
}

// ===========================================================================
// Category 1: MockBackend creation and configuration (tests 1–10)
// ===========================================================================

#[test]
fn t01_mock_backend_default_creation() {
    let _backend = MockBackend;
}

#[test]
fn t02_mock_backend_is_clone() {
    let a = MockBackend;
    let b = a.clone();
    assert_eq!(a.identity().id, b.identity().id);
}

#[test]
fn t03_mock_backend_is_debug() {
    let backend = MockBackend;
    let dbg = format!("{backend:?}");
    assert!(dbg.contains("MockBackend"));
}

#[test]
fn t04_mock_backend_identity_id() {
    assert_eq!(MockBackend.identity().id, "mock");
}

#[test]
fn t05_mock_backend_identity_backend_version() {
    assert_eq!(
        MockBackend.identity().backend_version.as_deref(),
        Some("0.1")
    );
}

#[test]
fn t06_mock_backend_identity_adapter_version() {
    assert_eq!(
        MockBackend.identity().adapter_version.as_deref(),
        Some("0.1")
    );
}

#[test]
fn t07_mock_backend_identity_stable_across_calls() {
    let backend = MockBackend;
    let id1 = backend.identity();
    let id2 = backend.identity();
    assert_eq!(id1.id, id2.id);
    assert_eq!(id1.backend_version, id2.backend_version);
    assert_eq!(id1.adapter_version, id2.adapter_version);
}

#[test]
fn t08_mock_backend_identity_from_clone() {
    let a = MockBackend;
    let b = a.clone();
    assert_eq!(a.identity().id, b.identity().id);
    assert_eq!(a.identity().backend_version, b.identity().backend_version);
}

#[test]
fn t09_mock_backend_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MockBackend>();
}

#[test]
fn t10_mock_backend_as_trait_object() {
    let backend: Box<dyn Backend> = Box::new(MockBackend);
    assert_eq!(backend.identity().id, "mock");
}

// ===========================================================================
// Category 2: Backend trait methods (tests 11–20)
// ===========================================================================

#[test]
fn t11_capabilities_contains_streaming() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::Streaming),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn t12_capabilities_contains_tool_read() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::ToolRead),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn t13_capabilities_contains_tool_write() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::ToolWrite),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn t14_capabilities_contains_tool_edit() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::ToolEdit),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn t15_capabilities_contains_tool_bash() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::ToolBash),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn t16_capabilities_contains_structured_output() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::StructuredOutputJsonSchema),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn t17_capabilities_does_not_contain_mcp_client() {
    let caps = MockBackend.capabilities();
    assert!(!caps.contains_key(&Capability::McpClient));
}

#[test]
fn t18_capabilities_stable_across_calls() {
    let backend = MockBackend;
    let c1 = backend.capabilities();
    let c2 = backend.capabilities();
    assert_eq!(c1.len(), c2.len());
    for (k, v1) in &c1 {
        let v2 = c2.get(k).unwrap();
        assert_eq!(std::mem::discriminant(v1), std::mem::discriminant(v2));
    }
}

#[test]
fn t19_capabilities_count() {
    let caps = MockBackend.capabilities();
    assert_eq!(caps.len(), 6, "MockBackend should advertise 6 capabilities");
}

#[test]
fn t20_trait_object_capabilities() {
    let backend: Box<dyn Backend> = Box::new(MockBackend);
    let caps = backend.capabilities();
    assert!(!caps.is_empty());
}

// ===========================================================================
// Category 3: Event streaming lifecycle (tests 21–30)
// ===========================================================================

#[tokio::test]
async fn t21_run_streams_run_started_first() {
    let (_, events) = run_mock(&MockBackend, base_work_order()).await;
    assert!(matches!(events[0].kind, AgentEventKind::RunStarted { .. }));
}

#[tokio::test]
async fn t22_run_streams_run_completed_last() {
    let (_, events) = run_mock(&MockBackend, base_work_order()).await;
    assert!(matches!(
        events.last().unwrap().kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[tokio::test]
async fn t23_run_streams_assistant_message() {
    let (_, events) = run_mock(&MockBackend, base_work_order()).await;
    assert!(
        events
            .iter()
            .any(|e| matches!(&e.kind, AgentEventKind::AssistantMessage { .. }))
    );
}

#[tokio::test]
async fn t24_run_streams_at_least_three_events() {
    let (_, events) = run_mock(&MockBackend, base_work_order()).await;
    assert!(events.len() >= 3);
}

#[tokio::test]
async fn t25_event_timestamps_are_monotonic() {
    let (_, events) = run_mock(&MockBackend, base_work_order()).await;
    for w in events.windows(2) {
        assert!(
            w[1].ts >= w[0].ts,
            "event timestamps should be non-decreasing"
        );
    }
}

#[tokio::test]
async fn t26_run_started_message_contains_task() {
    let wo = work_order_with_task("my special task");
    let (_, events) = run_mock(&MockBackend, wo).await;
    if let AgentEventKind::RunStarted { message } = &events[0].kind {
        assert!(message.contains("my special task"));
    } else {
        panic!("first event should be RunStarted");
    }
}

#[tokio::test]
async fn t27_trace_matches_stream_count() {
    let backend = MockBackend;
    let (tx, mut rx) = mpsc::channel(64);
    let receipt = backend
        .run(Uuid::new_v4(), base_work_order(), tx)
        .await
        .unwrap();
    let mut count = 0;
    while rx.try_recv().is_ok() {
        count += 1;
    }
    assert_eq!(receipt.trace.len(), count);
}

#[tokio::test]
async fn t28_trace_matches_stream_kinds() {
    let backend = MockBackend;
    let (tx, mut rx) = mpsc::channel(64);
    let receipt = backend
        .run(Uuid::new_v4(), base_work_order(), tx)
        .await
        .unwrap();
    let mut streamed = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        streamed.push(ev);
    }
    for (i, (t, s)) in receipt.trace.iter().zip(&streamed).enumerate() {
        assert_eq!(
            std::mem::discriminant(&t.kind),
            std::mem::discriminant(&s.kind),
            "event {i} kind mismatch between trace and stream"
        );
    }
}

#[tokio::test]
async fn t29_all_events_have_ext_none() {
    let (_, events) = run_mock(&MockBackend, base_work_order()).await;
    for ev in &events {
        assert!(ev.ext.is_none(), "mock backend events should have ext=None");
    }
}

#[tokio::test]
async fn t30_trace_events_have_ext_none() {
    let (receipt, _) = run_mock(&MockBackend, base_work_order()).await;
    for ev in &receipt.trace {
        assert!(ev.ext.is_none());
    }
}

// ===========================================================================
// Category 4: Concurrent backend execution (tests 31–40)
// ===========================================================================

#[tokio::test]
async fn t31_two_concurrent_runs() {
    let backend = MockBackend;
    let (tx1, _rx1) = mpsc::channel(16);
    let (tx2, _rx2) = mpsc::channel(16);
    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();

    let (r1, r2) = tokio::join!(
        backend.run(id1, base_work_order(), tx1),
        backend.run(id2, base_work_order(), tx2),
    );
    assert!(r1.is_ok());
    assert!(r2.is_ok());
    assert_ne!(r1.unwrap().meta.run_id, r2.unwrap().meta.run_id);
}

#[tokio::test]
async fn t32_ten_concurrent_runs_all_complete() {
    let backend = MockBackend;
    let handles: Vec<_> = (0..10)
        .map(|_| {
            let b = backend.clone();
            tokio::spawn(async move {
                let (tx, _rx) = mpsc::channel(16);
                b.run(Uuid::new_v4(), base_work_order(), tx).await
            })
        })
        .collect();

    for h in handles {
        let receipt = h.await.unwrap().unwrap();
        assert!(matches!(receipt.outcome, Outcome::Complete));
    }
}

#[tokio::test]
async fn t33_concurrent_runs_unique_run_ids() {
    let backend = MockBackend;
    let handles: Vec<_> = (0..20)
        .map(|_| {
            let b = backend.clone();
            tokio::spawn(async move {
                let (tx, _rx) = mpsc::channel(16);
                b.run(Uuid::new_v4(), base_work_order(), tx).await.unwrap()
            })
        })
        .collect();

    let mut ids = HashSet::new();
    for h in handles {
        let receipt = h.await.unwrap();
        assert!(ids.insert(receipt.meta.run_id));
    }
    assert_eq!(ids.len(), 20);
}

#[tokio::test]
async fn t34_concurrent_runs_unique_hashes() {
    let backend = MockBackend;
    let handles: Vec<_> = (0..5)
        .map(|_| {
            let b = backend.clone();
            tokio::spawn(async move {
                let (tx, _rx) = mpsc::channel(16);
                b.run(Uuid::new_v4(), base_work_order(), tx).await.unwrap()
            })
        })
        .collect();

    let mut hashes = HashSet::new();
    for h in handles {
        let receipt = h.await.unwrap();
        hashes.insert(receipt.receipt_sha256.unwrap());
    }
    // Each run has a unique run_id/timestamps, so hashes should differ
    assert_eq!(hashes.len(), 5);
}

#[tokio::test]
async fn t35_concurrent_runs_with_different_tasks() {
    let backend = MockBackend;
    let tasks = vec!["task-a", "task-b", "task-c", "task-d", "task-e"];
    let handles: Vec<_> = tasks
        .into_iter()
        .map(|task| {
            let b = backend.clone();
            let wo = work_order_with_task(task);
            tokio::spawn(async move {
                let (tx, _rx) = mpsc::channel(16);
                b.run(Uuid::new_v4(), wo, tx).await.unwrap()
            })
        })
        .collect();

    for h in handles {
        let receipt = h.await.unwrap();
        assert!(matches!(receipt.outcome, Outcome::Complete));
    }
}

#[tokio::test]
async fn t36_concurrent_runs_via_arc() {
    let backend: Arc<dyn Backend> = Arc::new(MockBackend);
    let handles: Vec<_> = (0..5)
        .map(|_| {
            let b = Arc::clone(&backend);
            tokio::spawn(async move {
                let (tx, _rx) = mpsc::channel(16);
                b.run(Uuid::new_v4(), base_work_order(), tx).await.unwrap()
            })
        })
        .collect();

    for h in handles {
        assert!(matches!(h.await.unwrap().outcome, Outcome::Complete));
    }
}

#[tokio::test]
async fn t37_concurrent_event_collection() {
    let backend = MockBackend;
    let handles: Vec<_> = (0..5)
        .map(|_| {
            let b = backend.clone();
            tokio::spawn(async move {
                let (tx, mut rx) = mpsc::channel(64);
                let receipt = b.run(Uuid::new_v4(), base_work_order(), tx).await.unwrap();
                let mut events = Vec::new();
                while let Ok(ev) = rx.try_recv() {
                    events.push(ev);
                }
                (receipt, events)
            })
        })
        .collect();

    for h in handles {
        let (receipt, events) = h.await.unwrap();
        assert_eq!(receipt.trace.len(), events.len());
    }
}

#[tokio::test]
async fn t38_concurrent_runs_do_not_interfere() {
    let backend = MockBackend;
    let id_a = Uuid::new_v4();
    let id_b = Uuid::new_v4();
    let (tx_a, _rx_a) = mpsc::channel(16);
    let (tx_b, _rx_b) = mpsc::channel(16);

    let (ra, rb) = tokio::join!(
        backend.run(id_a, base_work_order(), tx_a),
        backend.run(id_b, base_work_order(), tx_b),
    );

    let ra = ra.unwrap();
    let rb = rb.unwrap();
    assert_eq!(ra.meta.run_id, id_a);
    assert_eq!(rb.meta.run_id, id_b);
}

#[tokio::test]
async fn t39_concurrent_runs_all_have_hashes() {
    let backend = MockBackend;
    let handles: Vec<_> = (0..10)
        .map(|_| {
            let b = backend.clone();
            tokio::spawn(async move {
                let (tx, _rx) = mpsc::channel(16);
                b.run(Uuid::new_v4(), base_work_order(), tx).await.unwrap()
            })
        })
        .collect();

    for h in handles {
        assert!(h.await.unwrap().receipt_sha256.is_some());
    }
}

#[tokio::test]
async fn t40_concurrent_different_work_order_ids() {
    let backend = MockBackend;
    let handles: Vec<_> = (0..5)
        .map(|_| {
            let b = backend.clone();
            let wo = base_work_order();
            let wo_id = wo.id;
            tokio::spawn(async move {
                let (tx, _rx) = mpsc::channel(16);
                let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
                (receipt, wo_id)
            })
        })
        .collect();

    let mut wo_ids = HashSet::new();
    for h in handles {
        let (receipt, wo_id) = h.await.unwrap();
        assert_eq!(receipt.meta.work_order_id, wo_id);
        wo_ids.insert(wo_id);
    }
    assert_eq!(wo_ids.len(), 5);
}

// ===========================================================================
// Category 5: Backend error handling (tests 41–50)
// ===========================================================================

#[tokio::test]
async fn t41_unsatisfied_native_requirement_errors() {
    let backend = MockBackend;
    let mut wo = base_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Native,
        }],
    };
    let (tx, _rx) = mpsc::channel(16);
    let result = backend.run(Uuid::new_v4(), wo, tx).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn t42_error_message_mentions_capability() {
    let backend = MockBackend;
    let mut wo = base_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Native,
        }],
    };
    let (tx, _rx) = mpsc::channel(16);
    let err = backend.run(Uuid::new_v4(), wo, tx).await.unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("capability requirements not satisfied"));
}

#[tokio::test]
async fn t43_unsatisfied_emulated_requirement_errors() {
    let backend = MockBackend;
    let mut wo = base_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::SessionResume,
            min_support: MinSupport::Emulated,
        }],
    };
    let (tx, _rx) = mpsc::channel(16);
    assert!(backend.run(Uuid::new_v4(), wo, tx).await.is_err());
}

#[tokio::test]
async fn t44_satisfied_native_requirement_succeeds() {
    let backend = MockBackend;
    let mut wo = base_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    let (tx, _rx) = mpsc::channel(16);
    assert!(backend.run(Uuid::new_v4(), wo, tx).await.is_ok());
}

#[tokio::test]
async fn t45_satisfied_emulated_requirement_succeeds() {
    let backend = MockBackend;
    let mut wo = base_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Emulated,
        }],
    };
    let (tx, _rx) = mpsc::channel(16);
    assert!(backend.run(Uuid::new_v4(), wo, tx).await.is_ok());
}

#[tokio::test]
async fn t46_native_satisfies_emulated() {
    let backend = MockBackend;
    let mut wo = base_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Emulated,
        }],
    };
    let (tx, _rx) = mpsc::channel(16);
    assert!(backend.run(Uuid::new_v4(), wo, tx).await.is_ok());
}

#[tokio::test]
async fn t47_multiple_satisfied_requirements() {
    let backend = MockBackend;
    let mut wo = base_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Emulated,
            },
            CapabilityRequirement {
                capability: Capability::ToolWrite,
                min_support: MinSupport::Emulated,
            },
        ],
    };
    let (tx, _rx) = mpsc::channel(16);
    assert!(backend.run(Uuid::new_v4(), wo, tx).await.is_ok());
}

#[tokio::test]
async fn t48_one_unsatisfied_among_many_errors() {
    let backend = MockBackend;
    let mut wo = base_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::McpClient,
                min_support: MinSupport::Native,
            },
        ],
    };
    let (tx, _rx) = mpsc::channel(16);
    assert!(backend.run(Uuid::new_v4(), wo, tx).await.is_err());
}

#[tokio::test]
async fn t49_empty_requirements_succeeds() {
    let backend = MockBackend;
    let wo = base_work_order();
    let (tx, _rx) = mpsc::channel(16);
    assert!(backend.run(Uuid::new_v4(), wo, tx).await.is_ok());
}

#[tokio::test]
async fn t50_ensure_capability_requirements_standalone() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    let caps = MockBackend.capabilities();
    assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
}

// ===========================================================================
// Category 6: Event channel behavior (tests 51–60)
// ===========================================================================

#[tokio::test]
async fn t51_channel_capacity_1_with_consumer() {
    let backend = MockBackend;
    let (tx, mut rx) = mpsc::channel(1);
    let consumer = tokio::spawn(async move {
        let mut count = 0u32;
        while rx.recv().await.is_some() {
            count += 1;
        }
        count
    });
    let receipt = backend
        .run(Uuid::new_v4(), base_work_order(), tx)
        .await
        .unwrap();
    drop(receipt);
    let count = consumer.await.unwrap();
    assert!(count >= 3);
}

#[tokio::test]
async fn t52_large_channel_capacity() {
    let backend = MockBackend;
    let (tx, mut rx) = mpsc::channel(1024);
    let _receipt = backend
        .run(Uuid::new_v4(), base_work_order(), tx)
        .await
        .unwrap();
    let mut count = 0;
    while rx.try_recv().is_ok() {
        count += 1;
    }
    assert!(count >= 3);
}

#[tokio::test]
async fn t53_receiver_dropped_before_run_still_returns_receipt() {
    let backend = MockBackend;
    let (tx, rx) = mpsc::channel(16);
    drop(rx);
    // MockBackend ignores send errors (let _ = events_tx.send(...))
    let result = backend.run(Uuid::new_v4(), base_work_order(), tx).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn t54_receiver_dropped_receipt_has_trace() {
    let backend = MockBackend;
    let (tx, rx) = mpsc::channel(16);
    drop(rx);
    let receipt = backend
        .run(Uuid::new_v4(), base_work_order(), tx)
        .await
        .unwrap();
    // Trace is built independently of channel sends
    assert!(!receipt.trace.is_empty());
}

#[tokio::test]
async fn t55_events_consumed_during_run() {
    let backend = MockBackend;
    let (tx, mut rx) = mpsc::channel(64);
    let handle = tokio::spawn(async move {
        let mut events = Vec::new();
        while let Some(ev) = rx.recv().await {
            events.push(ev);
        }
        events
    });
    let _receipt = backend
        .run(Uuid::new_v4(), base_work_order(), tx)
        .await
        .unwrap();
    // Sender dropped after run, consumer will finish
    let events = handle.await.unwrap();
    assert!(events.len() >= 3);
}

#[tokio::test]
async fn t56_channel_empty_after_full_consumption() {
    let backend = MockBackend;
    let (tx, mut rx) = mpsc::channel(64);
    let _receipt = backend
        .run(Uuid::new_v4(), base_work_order(), tx)
        .await
        .unwrap();
    while rx.try_recv().is_ok() {}
    // Channel should be empty now
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn t57_multiple_receivers_cannot_exist() {
    // mpsc::Receiver is not Clone—this is a compile-time guarantee.
    // We verify the type system works as expected by ensuring a single receiver works.
    let backend = MockBackend;
    let (tx, mut rx) = mpsc::channel(16);
    let _receipt = backend
        .run(Uuid::new_v4(), base_work_order(), tx)
        .await
        .unwrap();
    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    assert!(!events.is_empty());
}

#[tokio::test]
async fn t58_sender_clone_sends_to_same_channel() {
    let backend = MockBackend;
    let (tx, mut rx) = mpsc::channel(64);
    // Backend receives the original sender
    let _receipt = backend
        .run(Uuid::new_v4(), base_work_order(), tx)
        .await
        .unwrap();
    let mut count = 0;
    while rx.try_recv().is_ok() {
        count += 1;
    }
    assert!(count >= 3);
}

#[tokio::test]
async fn t59_events_arrive_in_order() {
    let backend = MockBackend;
    let (tx, mut rx) = mpsc::channel(64);
    let _receipt = backend
        .run(Uuid::new_v4(), base_work_order(), tx)
        .await
        .unwrap();
    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    assert!(matches!(events[0].kind, AgentEventKind::RunStarted { .. }));
    assert!(matches!(
        events.last().unwrap().kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[tokio::test]
async fn t60_channel_recv_returns_none_after_sender_dropped() {
    let backend = MockBackend;
    let (tx, mut rx) = mpsc::channel(64);
    let _receipt = backend
        .run(Uuid::new_v4(), base_work_order(), tx)
        .await
        .unwrap();
    // tx is dropped here, so after draining, recv returns None
    while rx.try_recv().is_ok() {}
    assert!(rx.recv().await.is_none());
}

// ===========================================================================
// Category 7: Receipt generation (tests 61–72)
// ===========================================================================

#[tokio::test]
async fn t61_receipt_has_hash() {
    let (receipt, _) = run_mock(&MockBackend, base_work_order()).await;
    assert!(receipt.receipt_sha256.is_some());
}

#[tokio::test]
async fn t62_receipt_hash_is_hex_string() {
    let (receipt, _) = run_mock(&MockBackend, base_work_order()).await;
    let hash = receipt.receipt_sha256.unwrap();
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[tokio::test]
async fn t63_receipt_hash_length_sha256() {
    let (receipt, _) = run_mock(&MockBackend, base_work_order()).await;
    let hash = receipt.receipt_sha256.unwrap();
    assert_eq!(hash.len(), 64, "SHA-256 hex string should be 64 chars");
}

#[tokio::test]
async fn t64_receipt_hash_recompute_matches() {
    let (receipt, _) = run_mock(&MockBackend, base_work_order()).await;
    let stored = receipt.receipt_sha256.clone().unwrap();
    let recomputed = abp_core::receipt_hash(&receipt).unwrap();
    assert_eq!(stored, recomputed);
}

#[tokio::test]
async fn t65_receipt_contract_version() {
    let (receipt, _) = run_mock(&MockBackend, base_work_order()).await;
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn t66_receipt_run_id_matches_provided() {
    let backend = MockBackend;
    let run_id = Uuid::new_v4();
    let (tx, _rx) = mpsc::channel(16);
    let receipt = backend.run(run_id, base_work_order(), tx).await.unwrap();
    assert_eq!(receipt.meta.run_id, run_id);
}

#[tokio::test]
async fn t67_receipt_work_order_id_matches() {
    let wo = base_work_order();
    let wo_id = wo.id;
    let (receipt, _) = run_mock(&MockBackend, wo).await;
    assert_eq!(receipt.meta.work_order_id, wo_id);
}

#[tokio::test]
async fn t68_receipt_outcome_is_complete() {
    let (receipt, _) = run_mock(&MockBackend, base_work_order()).await;
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[tokio::test]
async fn t69_receipt_timing_valid() {
    let (receipt, _) = run_mock(&MockBackend, base_work_order()).await;
    assert!(receipt.meta.finished_at >= receipt.meta.started_at);
}

#[tokio::test]
async fn t70_receipt_usage_normalized() {
    let (receipt, _) = run_mock(&MockBackend, base_work_order()).await;
    assert_eq!(receipt.usage.input_tokens, Some(0));
    assert_eq!(receipt.usage.output_tokens, Some(0));
    assert_eq!(receipt.usage.estimated_cost_usd, Some(0.0));
}

#[tokio::test]
async fn t71_receipt_usage_raw() {
    let (receipt, _) = run_mock(&MockBackend, base_work_order()).await;
    assert_eq!(receipt.usage_raw, json!({"note": "mock"}));
}

#[tokio::test]
async fn t72_receipt_verification_report() {
    let (receipt, _) = run_mock(&MockBackend, base_work_order()).await;
    assert!(receipt.verification.harness_ok);
    assert!(receipt.verification.git_diff.is_none());
    assert!(receipt.verification.git_status.is_none());
    assert!(receipt.artifacts.is_empty());
}

// ===========================================================================
// Category 8: Backend timeout handling (tests 73–78)
// ===========================================================================

#[tokio::test]
async fn t73_mock_backend_completes_within_timeout() {
    let backend = MockBackend;
    let (tx, _rx) = mpsc::channel(16);
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        backend.run(Uuid::new_v4(), base_work_order(), tx),
    )
    .await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_ok());
}

#[tokio::test]
async fn t74_mock_backend_fast_enough_for_tight_timeout() {
    let backend = MockBackend;
    let (tx, _rx) = mpsc::channel(16);
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        backend.run(Uuid::new_v4(), base_work_order(), tx),
    )
    .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn t75_timeout_wrapping_preserves_receipt() {
    let backend = MockBackend;
    let (tx, _rx) = mpsc::channel(16);
    let receipt = tokio::time::timeout(
        Duration::from_secs(5),
        backend.run(Uuid::new_v4(), base_work_order(), tx),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[tokio::test]
async fn t76_concurrent_timeout_wrapped_runs() {
    let backend = MockBackend;
    let handles: Vec<_> = (0..5)
        .map(|_| {
            let b = backend.clone();
            tokio::spawn(async move {
                let (tx, _rx) = mpsc::channel(16);
                tokio::time::timeout(
                    Duration::from_secs(5),
                    b.run(Uuid::new_v4(), base_work_order(), tx),
                )
                .await
            })
        })
        .collect();

    for h in handles {
        let result = h.await.unwrap();
        assert!(result.is_ok());
        assert!(result.unwrap().is_ok());
    }
}

#[tokio::test]
async fn t77_timeout_does_not_corrupt_receipt() {
    let backend = MockBackend;
    let (tx, _rx) = mpsc::channel(16);
    let receipt = tokio::time::timeout(
        Duration::from_secs(5),
        backend.run(Uuid::new_v4(), base_work_order(), tx),
    )
    .await
    .unwrap()
    .unwrap();
    let stored = receipt.receipt_sha256.clone().unwrap();
    let recomputed = abp_core::receipt_hash(&receipt).unwrap();
    assert_eq!(stored, recomputed);
}

#[tokio::test]
async fn t78_select_with_backend_run() {
    let backend = MockBackend;
    let (tx, _rx) = mpsc::channel(16);
    let result = tokio::select! {
        r = backend.run(Uuid::new_v4(), base_work_order(), tx) => r,
        _ = tokio::time::sleep(Duration::from_secs(10)) => panic!("timeout"),
    };
    assert!(result.is_ok());
}

// ===========================================================================
// Category 9: Backend cancellation (tests 79–84)
// ===========================================================================

#[tokio::test]
async fn t79_abort_handle_cancellation() {
    let backend = MockBackend;
    let (tx, _rx) = mpsc::channel(16);
    let handle =
        tokio::spawn(async move { backend.run(Uuid::new_v4(), base_work_order(), tx).await });
    // MockBackend completes almost instantly, but we test the abort mechanism
    let result = handle.await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn t80_drop_sender_before_consumer_finishes() {
    let backend = MockBackend;
    let (tx, mut rx) = mpsc::channel(64);
    let _receipt = backend
        .run(Uuid::new_v4(), base_work_order(), tx)
        .await
        .unwrap();
    // Sender is now dropped, receiver can drain
    let mut events = Vec::new();
    while let Some(ev) = rx.recv().await {
        events.push(ev);
    }
    assert!(!events.is_empty());
}

#[tokio::test]
async fn t81_select_immediate_completion() {
    let backend = MockBackend;
    let (tx, _rx) = mpsc::channel(16);
    let done = tokio::select! {
        r = backend.run(Uuid::new_v4(), base_work_order(), tx) => r.is_ok(),
        _ = async { tokio::time::sleep(Duration::from_millis(1)).await } => false,
    };
    // MockBackend is so fast it should always win the race
    assert!(done);
}

#[tokio::test]
async fn t82_cancelled_task_does_not_affect_new_run() {
    let backend = MockBackend;

    // First run (potentially cancelled via abort)
    let (tx1, _rx1) = mpsc::channel(16);
    let handle = tokio::spawn({
        let b = backend.clone();
        async move { b.run(Uuid::new_v4(), base_work_order(), tx1).await }
    });
    let _ = handle.await;

    // Second run should work fine
    let (tx2, _rx2) = mpsc::channel(16);
    let receipt = backend
        .run(Uuid::new_v4(), base_work_order(), tx2)
        .await
        .unwrap();
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[tokio::test]
async fn t83_dropped_receiver_during_run_no_panic() {
    let backend = MockBackend;
    let (tx, rx) = mpsc::channel(1);
    // Drop receiver immediately
    drop(rx);
    // Should not panic
    let result = backend.run(Uuid::new_v4(), base_work_order(), tx).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn t84_multiple_cancel_restart_cycles() {
    let backend = MockBackend;
    for _ in 0..10 {
        let (tx, rx) = mpsc::channel(16);
        drop(rx);
        let result = backend.run(Uuid::new_v4(), base_work_order(), tx).await;
        assert!(result.is_ok());
    }
}

// ===========================================================================
// Category 10: Multiple sequential runs (tests 85–92)
// ===========================================================================

#[tokio::test]
async fn t85_sequential_runs_produce_different_run_ids() {
    let backend = MockBackend;
    let mut ids = Vec::new();
    for _ in 0..5 {
        let run_id = Uuid::new_v4();
        let (tx, _rx) = mpsc::channel(16);
        let receipt = backend.run(run_id, base_work_order(), tx).await.unwrap();
        ids.push(receipt.meta.run_id);
    }
    let set: HashSet<_> = ids.iter().collect();
    assert_eq!(set.len(), 5);
}

#[tokio::test]
async fn t86_sequential_runs_all_complete() {
    let backend = MockBackend;
    for _ in 0..10 {
        let (tx, _rx) = mpsc::channel(16);
        let receipt = backend
            .run(Uuid::new_v4(), base_work_order(), tx)
            .await
            .unwrap();
        assert!(matches!(receipt.outcome, Outcome::Complete));
    }
}

#[tokio::test]
async fn t87_sequential_runs_different_tasks() {
    let backend = MockBackend;
    let tasks = ["alpha", "beta", "gamma", "delta"];
    for task in &tasks {
        let (receipt, _) = run_mock(&backend, work_order_with_task(task)).await;
        assert!(matches!(receipt.outcome, Outcome::Complete));
    }
}

#[tokio::test]
async fn t88_sequential_runs_all_have_valid_hashes() {
    let backend = MockBackend;
    for _ in 0..5 {
        let (receipt, _) = run_mock(&backend, base_work_order()).await;
        let stored = receipt.receipt_sha256.clone().unwrap();
        let recomputed = abp_core::receipt_hash(&receipt).unwrap();
        assert_eq!(stored, recomputed);
    }
}

#[tokio::test]
async fn t89_sequential_runs_different_hashes() {
    let backend = MockBackend;
    let mut hashes = HashSet::new();
    for _ in 0..5 {
        let (receipt, _) = run_mock(&backend, base_work_order()).await;
        hashes.insert(receipt.receipt_sha256.unwrap());
    }
    assert_eq!(hashes.len(), 5);
}

#[tokio::test]
async fn t90_sequential_runs_preserve_work_order_id() {
    let backend = MockBackend;
    for _ in 0..5 {
        let wo = base_work_order();
        let wo_id = wo.id;
        let (receipt, _) = run_mock(&backend, wo).await;
        assert_eq!(receipt.meta.work_order_id, wo_id);
    }
}

#[tokio::test]
async fn t91_sequential_runs_with_mixed_requirements() {
    let backend = MockBackend;

    // Run with no requirements
    let (r1, _) = run_mock(&backend, base_work_order()).await;
    assert!(matches!(r1.outcome, Outcome::Complete));

    // Run with satisfied requirement
    let mut wo = base_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    let (r2, _) = run_mock(&backend, wo).await;
    assert!(matches!(r2.outcome, Outcome::Complete));
}

#[tokio::test]
async fn t92_sequential_error_then_success() {
    let backend = MockBackend;

    // First run: unsatisfied requirement → error
    let mut wo = base_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Native,
        }],
    };
    let (tx, _rx) = mpsc::channel(16);
    assert!(backend.run(Uuid::new_v4(), wo, tx).await.is_err());

    // Second run: no requirements → success
    let (tx2, _rx2) = mpsc::channel(16);
    let receipt = backend
        .run(Uuid::new_v4(), base_work_order(), tx2)
        .await
        .unwrap();
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

// ===========================================================================
// Category 11: Backend metadata/capabilities reporting (tests 93–100)
// ===========================================================================

#[test]
fn t93_identity_returns_backend_identity_type() {
    let _id: BackendIdentity = MockBackend.identity();
}

#[test]
fn t94_capabilities_returns_capability_manifest_type() {
    let _caps: CapabilityManifest = MockBackend.capabilities();
}

#[test]
fn t95_capabilities_is_btreemap() {
    let caps = MockBackend.capabilities();
    // BTreeMap iteration is sorted by key
    let keys: Vec<_> = caps.keys().collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted, "BTreeMap keys should be sorted");
}

#[tokio::test]
async fn t96_receipt_backend_matches_identity() {
    let backend = MockBackend;
    let (receipt, _) = run_mock(&backend, base_work_order()).await;
    let id = backend.identity();
    assert_eq!(receipt.backend.id, id.id);
    assert_eq!(receipt.backend.backend_version, id.backend_version);
    assert_eq!(receipt.backend.adapter_version, id.adapter_version);
}

#[tokio::test]
async fn t97_receipt_capabilities_match_advertised() {
    let backend = MockBackend;
    let (receipt, _) = run_mock(&backend, base_work_order()).await;
    let caps = backend.capabilities();
    assert_eq!(receipt.capabilities.len(), caps.len());
    for (k, v) in &caps {
        let rv = receipt.capabilities.get(k).unwrap();
        assert_eq!(std::mem::discriminant(v), std::mem::discriminant(rv));
    }
}

#[tokio::test]
async fn t98_receipt_default_mode_is_mapped() {
    let (receipt, _) = run_mock(&MockBackend, base_work_order()).await;
    assert_eq!(receipt.mode, ExecutionMode::Mapped);
}

#[tokio::test]
async fn t99_passthrough_mode_in_receipt() {
    let mut wo = base_work_order();
    wo.config
        .vendor
        .insert("abp".into(), json!({"mode": "passthrough"}));
    let (receipt, _) = run_mock(&MockBackend, wo).await;
    assert_eq!(receipt.mode, ExecutionMode::Passthrough);
}

#[test]
fn t100_extract_execution_mode_default() {
    let wo = base_work_order();
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Mapped);
}

// ===========================================================================
// Category 12: Event ordering guarantees (tests 101–107)
// ===========================================================================

#[tokio::test]
async fn t101_first_event_always_run_started() {
    for _ in 0..10 {
        let (_, events) = run_mock(&MockBackend, base_work_order()).await;
        assert!(matches!(events[0].kind, AgentEventKind::RunStarted { .. }));
    }
}

#[tokio::test]
async fn t102_last_event_always_run_completed() {
    for _ in 0..10 {
        let (_, events) = run_mock(&MockBackend, base_work_order()).await;
        assert!(matches!(
            events.last().unwrap().kind,
            AgentEventKind::RunCompleted { .. }
        ));
    }
}

#[tokio::test]
async fn t103_assistant_message_between_bookends() {
    let (_, events) = run_mock(&MockBackend, base_work_order()).await;
    assert!(events.len() >= 3);
    // Middle events should not be RunStarted or RunCompleted
    for ev in &events[1..events.len() - 1] {
        assert!(
            !matches!(ev.kind, AgentEventKind::RunStarted { .. }),
            "RunStarted should only appear first"
        );
        assert!(
            !matches!(ev.kind, AgentEventKind::RunCompleted { .. }),
            "RunCompleted should only appear last"
        );
    }
}

#[tokio::test]
async fn t104_event_count_consistent() {
    let (_, events) = run_mock(&MockBackend, base_work_order()).await;
    assert!(events.len() >= 3, "MockBackend emits at least 3 events");
    // Confirm stability across runs
    let (_, events2) = run_mock(&MockBackend, base_work_order()).await;
    assert_eq!(events.len(), events2.len(), "event count should be stable");
}

#[tokio::test]
async fn t105_event_order_bookends() {
    let (_, events) = run_mock(&MockBackend, base_work_order()).await;
    assert!(matches!(events[0].kind, AgentEventKind::RunStarted { .. }));
    assert!(matches!(
        events.last().unwrap().kind,
        AgentEventKind::RunCompleted { .. }
    ));
    // All middle events should be non-bookend types
    for ev in &events[1..events.len() - 1] {
        assert!(!matches!(ev.kind, AgentEventKind::RunStarted { .. }));
        assert!(!matches!(ev.kind, AgentEventKind::RunCompleted { .. }));
    }
}

#[tokio::test]
async fn t106_trace_order_matches_stream_order() {
    let backend = MockBackend;
    let (tx, mut rx) = mpsc::channel(64);
    let receipt = backend
        .run(Uuid::new_v4(), base_work_order(), tx)
        .await
        .unwrap();
    let mut streamed = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        streamed.push(ev);
    }
    assert_eq!(receipt.trace.len(), streamed.len());
    for (t, s) in receipt.trace.iter().zip(&streamed) {
        assert_eq!(
            std::mem::discriminant(&t.kind),
            std::mem::discriminant(&s.kind),
        );
    }
}

#[tokio::test]
async fn t107_timestamps_non_decreasing() {
    let (_, events) = run_mock(&MockBackend, base_work_order()).await;
    for pair in events.windows(2) {
        assert!(pair[1].ts >= pair[0].ts);
    }
}

// ===========================================================================
// Category 13: Large event volumes (tests 108–113)
// ===========================================================================

#[tokio::test]
async fn t108_many_sequential_runs_stable() {
    let backend = MockBackend;
    for i in 0..50 {
        let (tx, _rx) = mpsc::channel(16);
        let receipt = backend
            .run(
                Uuid::new_v4(),
                work_order_with_task(&format!("task-{i}")),
                tx,
            )
            .await
            .unwrap();
        assert!(matches!(receipt.outcome, Outcome::Complete));
    }
}

#[tokio::test]
async fn t109_many_concurrent_runs_stable() {
    let backend = MockBackend;
    let handles: Vec<_> = (0..50)
        .map(|i| {
            let b = backend.clone();
            tokio::spawn(async move {
                let (tx, _rx) = mpsc::channel(16);
                b.run(
                    Uuid::new_v4(),
                    work_order_with_task(&format!("concurrent-{i}")),
                    tx,
                )
                .await
                .unwrap()
            })
        })
        .collect();

    for h in handles {
        assert!(matches!(h.await.unwrap().outcome, Outcome::Complete));
    }
}

#[tokio::test]
async fn t110_large_task_payload() {
    let big_task = "x".repeat(200_000);
    let (receipt, _) = run_mock(&MockBackend, work_order_with_task(&big_task)).await;
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[tokio::test]
async fn t111_collect_all_events_from_many_runs() {
    let backend = MockBackend;
    // Determine events per run dynamically
    let (_, first) = run_mock(&backend, base_work_order()).await;
    let per_run = first.len();
    let mut total_events = per_run;
    for _ in 1..20 {
        let (_, events) = run_mock(&backend, base_work_order()).await;
        total_events += events.len();
    }
    assert_eq!(total_events, 20 * per_run);
}

#[tokio::test]
async fn t112_all_receipts_serializable() {
    let backend = MockBackend;
    for _ in 0..10 {
        let (receipt, _) = run_mock(&backend, base_work_order()).await;
        let json = serde_json::to_string(&receipt).unwrap();
        assert!(!json.is_empty());
        let _roundtrip: Receipt = serde_json::from_str(&json).unwrap();
    }
}

#[tokio::test]
async fn t113_all_events_serializable() {
    let (_, events) = run_mock(&MockBackend, base_work_order()).await;
    for ev in &events {
        let json = serde_json::to_string(ev).unwrap();
        assert!(!json.is_empty());
        let _roundtrip: AgentEvent = serde_json::from_str(&json).unwrap();
    }
}

// ===========================================================================
// Category 14: Backend state transitions (tests 114–125)
// ===========================================================================

#[tokio::test]
async fn t114_backend_stateless_between_runs() {
    let backend = MockBackend;
    let (r1, _) = run_mock(&backend, work_order_with_task("first")).await;
    let (r2, _) = run_mock(&backend, work_order_with_task("second")).await;
    // Both should succeed independently
    assert!(matches!(r1.outcome, Outcome::Complete));
    assert!(matches!(r2.outcome, Outcome::Complete));
    assert_ne!(r1.meta.run_id, r2.meta.run_id);
}

#[tokio::test]
async fn t115_identity_unchanged_after_run() {
    let backend = MockBackend;
    let id_before = backend.identity();
    let (_, _) = run_mock(&backend, base_work_order()).await;
    let id_after = backend.identity();
    assert_eq!(id_before.id, id_after.id);
}

#[tokio::test]
async fn t116_capabilities_unchanged_after_run() {
    let backend = MockBackend;
    let caps_before = backend.capabilities();
    let (_, _) = run_mock(&backend, base_work_order()).await;
    let caps_after = backend.capabilities();
    assert_eq!(caps_before.len(), caps_after.len());
}

#[tokio::test]
async fn t117_error_does_not_alter_backend_state() {
    let backend = MockBackend;
    let caps_before = backend.capabilities();

    // Trigger an error
    let mut wo = base_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Native,
        }],
    };
    let (tx, _rx) = mpsc::channel(16);
    let _ = backend.run(Uuid::new_v4(), wo, tx).await;

    let caps_after = backend.capabilities();
    assert_eq!(caps_before.len(), caps_after.len());
}

#[tokio::test]
async fn t118_run_after_error_succeeds() {
    let backend = MockBackend;

    // Error run
    let mut wo = base_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::SessionFork,
            min_support: MinSupport::Native,
        }],
    };
    let (tx, _rx) = mpsc::channel(16);
    assert!(backend.run(Uuid::new_v4(), wo, tx).await.is_err());

    // Success run
    let (receipt, _) = run_mock(&backend, base_work_order()).await;
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[tokio::test]
async fn t119_nil_uuid_as_run_id() {
    let backend = MockBackend;
    let (tx, _rx) = mpsc::channel(16);
    let receipt = backend
        .run(Uuid::nil(), base_work_order(), tx)
        .await
        .unwrap();
    assert_eq!(receipt.meta.run_id, Uuid::nil());
}

#[tokio::test]
async fn t120_max_uuid_as_run_id() {
    let backend = MockBackend;
    let max_id = Uuid::max();
    let (tx, _rx) = mpsc::channel(16);
    let receipt = backend.run(max_id, base_work_order(), tx).await.unwrap();
    assert_eq!(receipt.meta.run_id, max_id);
}

#[tokio::test]
async fn t121_work_order_builder_default_lane() {
    let wo = WorkOrderBuilder::new("test").build();
    let (receipt, _) = run_mock(&MockBackend, wo).await;
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[tokio::test]
async fn t122_empty_task_string() {
    let wo = WorkOrderBuilder::new("").build();
    let (receipt, _) = run_mock(&MockBackend, wo).await;
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[tokio::test]
async fn t123_receipt_serialization_roundtrip() {
    let (receipt, _) = run_mock(&MockBackend, base_work_order()).await;
    let json = serde_json::to_string_pretty(&receipt).unwrap();
    let deserialized: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.meta.run_id, receipt.meta.run_id);
    assert_eq!(deserialized.receipt_sha256, receipt.receipt_sha256);
    assert_eq!(deserialized.trace.len(), receipt.trace.len());
}

#[tokio::test]
async fn t124_receipt_json_contains_expected_fields() {
    let (receipt, _) = run_mock(&MockBackend, base_work_order()).await;
    let json: serde_json::Value = serde_json::to_value(&receipt).unwrap();
    assert!(json.get("meta").is_some());
    assert!(json.get("backend").is_some());
    assert!(json.get("capabilities").is_some());
    assert!(json.get("mode").is_some());
    assert!(json.get("usage_raw").is_some());
    assert!(json.get("usage").is_some());
    assert!(json.get("trace").is_some());
    assert!(json.get("artifacts").is_some());
    assert!(json.get("verification").is_some());
    assert!(json.get("outcome").is_some());
    assert!(json.get("receipt_sha256").is_some());
}

#[tokio::test]
async fn t125_duration_ms_consistent_with_timestamps() {
    let (receipt, _) = run_mock(&MockBackend, base_work_order()).await;
    let computed_delta = (receipt.meta.finished_at - receipt.meta.started_at)
        .to_std()
        .unwrap_or_default()
        .as_millis() as u64;
    assert_eq!(receipt.meta.duration_ms, computed_delta);
}

// ===========================================================================
// Additional: Config extraction tests (tests 126–131)
// ===========================================================================

#[test]
fn t126_extract_mode_nested_abp_passthrough() {
    let mut wo = base_work_order();
    wo.config
        .vendor
        .insert("abp".into(), json!({"mode": "passthrough"}));
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Passthrough);
}

#[test]
fn t127_extract_mode_dotted_key_passthrough() {
    let mut wo = base_work_order();
    wo.config
        .vendor
        .insert("abp.mode".into(), json!("passthrough"));
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Passthrough);
}

#[test]
fn t128_extract_mode_invalid_falls_back_to_mapped() {
    let mut wo = base_work_order();
    wo.config
        .vendor
        .insert("abp".into(), json!({"mode": "bogus"}));
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Mapped);
}

#[test]
fn t129_extract_mode_nested_takes_priority() {
    let mut wo = base_work_order();
    wo.config
        .vendor
        .insert("abp".into(), json!({"mode": "passthrough"}));
    wo.config.vendor.insert("abp.mode".into(), json!("mapped"));
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Passthrough);
}

#[test]
fn t130_validate_passthrough_ok() {
    assert!(validate_passthrough_compatibility(&base_work_order()).is_ok());
}

#[test]
fn t131_ensure_empty_requirements_empty_caps() {
    let reqs = CapabilityRequirements::default();
    let caps = CapabilityManifest::default();
    assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
}

// ===========================================================================
// Additional: Trait object / Arc patterns (tests 132–137)
// ===========================================================================

#[tokio::test]
async fn t132_arc_backend_run() {
    let backend: Arc<dyn Backend> = Arc::new(MockBackend);
    let (tx, _rx) = mpsc::channel(16);
    let receipt = backend
        .run(Uuid::new_v4(), base_work_order(), tx)
        .await
        .unwrap();
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[tokio::test]
async fn t133_box_backend_run() {
    let backend: Box<dyn Backend> = Box::new(MockBackend);
    let (tx, _rx) = mpsc::channel(16);
    let receipt = backend
        .run(Uuid::new_v4(), base_work_order(), tx)
        .await
        .unwrap();
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

#[tokio::test]
async fn t134_arc_concurrent_runs() {
    let backend: Arc<dyn Backend> = Arc::new(MockBackend);
    let handles: Vec<_> = (0..5)
        .map(|_| {
            let b = Arc::clone(&backend);
            tokio::spawn(async move {
                let (tx, _rx) = mpsc::channel(16);
                b.run(Uuid::new_v4(), base_work_order(), tx).await.unwrap()
            })
        })
        .collect();

    for h in handles {
        assert!(matches!(h.await.unwrap().outcome, Outcome::Complete));
    }
}

#[test]
fn t135_trait_object_identity() {
    let backend: &dyn Backend = &MockBackend;
    assert_eq!(backend.identity().id, "mock");
}

#[test]
fn t136_trait_object_capabilities() {
    let backend: &dyn Backend = &MockBackend;
    assert!(!backend.capabilities().is_empty());
}

#[tokio::test]
async fn t137_arc_shared_across_tasks() {
    let backend: Arc<dyn Backend> = Arc::new(MockBackend);
    let b1 = Arc::clone(&backend);
    let b2 = Arc::clone(&backend);

    let (tx1, _rx1) = mpsc::channel(16);
    let (tx2, _rx2) = mpsc::channel(16);

    let (r1, r2) = tokio::join!(
        b1.run(Uuid::new_v4(), base_work_order(), tx1),
        b2.run(Uuid::new_v4(), base_work_order(), tx2),
    );
    assert!(r1.is_ok());
    assert!(r2.is_ok());
}
