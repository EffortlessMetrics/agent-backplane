// SPDX-License-Identifier: MIT OR Apache-2.0
//! Extended MockBackend tests: concurrency, edge cases, config extraction.

use abp_core::{
    AgentEventKind, CONTRACT_VERSION, Capability, CapabilityRequirement, CapabilityRequirements,
    ExecutionMode, MinSupport, Outcome, WorkOrderBuilder,
};
use abp_integrations::{
    Backend, MockBackend, extract_execution_mode, validate_passthrough_compatibility,
};
use serde_json::json;
use tokio::sync::mpsc;
use uuid::Uuid;

fn base_work_order() -> abp_core::WorkOrder {
    WorkOrderBuilder::new("hello").build()
}

// ---------------------------------------------------------------------------
// 1. Empty task produces valid receipt
// ---------------------------------------------------------------------------

#[tokio::test]
async fn empty_task_produces_receipt() {
    let backend = MockBackend;
    let wo = WorkOrderBuilder::new("").build();
    let (tx, _rx) = mpsc::channel(16);

    let receipt = backend.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert!(matches!(receipt.outcome, Outcome::Complete));
    assert!(receipt.receipt_sha256.is_some());
}

// ---------------------------------------------------------------------------
// 2. Large payload task
// ---------------------------------------------------------------------------

#[tokio::test]
async fn large_payload_task_succeeds() {
    let backend = MockBackend;
    let large_task = "x".repeat(100_000);
    let wo = WorkOrderBuilder::new(&large_task).build();
    let (tx, _rx) = mpsc::channel(16);

    let receipt = backend.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert!(matches!(receipt.outcome, Outcome::Complete));
}

// ---------------------------------------------------------------------------
// 3. Trace events match streamed events
// ---------------------------------------------------------------------------

#[tokio::test]
async fn trace_matches_streamed_events() {
    let backend = MockBackend;
    let wo = base_work_order();
    let (tx, mut rx) = mpsc::channel(32);

    let receipt = backend.run(Uuid::new_v4(), wo, tx).await.unwrap();

    let mut streamed = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        streamed.push(ev);
    }

    assert_eq!(
        receipt.trace.len(),
        streamed.len(),
        "trace and stream should have same event count"
    );
    for (i, (trace_ev, stream_ev)) in receipt.trace.iter().zip(&streamed).enumerate() {
        assert_eq!(
            std::mem::discriminant(&trace_ev.kind),
            std::mem::discriminant(&stream_ev.kind),
            "event {i} kind mismatch"
        );
    }
}

// ---------------------------------------------------------------------------
// 4. Concurrent runs produce independent receipts
// ---------------------------------------------------------------------------

#[tokio::test]
async fn concurrent_runs_independent() {
    let backend = MockBackend;
    let run_ids: Vec<Uuid> = (0..5).map(|_| Uuid::new_v4()).collect();

    let handles: Vec<_> = run_ids
        .iter()
        .map(|&rid| {
            let b = backend.clone();
            let wo = base_work_order();
            tokio::spawn(async move {
                let (tx, _rx) = mpsc::channel(16);
                b.run(rid, wo, tx).await.unwrap()
            })
        })
        .collect();

    let mut seen_ids = std::collections::HashSet::new();
    for h in handles {
        let receipt: abp_core::Receipt = h.await.unwrap();
        assert!(matches!(receipt.outcome, Outcome::Complete));
        assert!(
            seen_ids.insert(receipt.meta.run_id),
            "duplicate run_id in concurrent receipts"
        );
    }
    assert_eq!(seen_ids.len(), 5);
}

// ---------------------------------------------------------------------------
// 5. Receipt contract version matches constant
// ---------------------------------------------------------------------------

#[tokio::test]
async fn receipt_contract_version() {
    let backend = MockBackend;
    let (tx, _rx) = mpsc::channel(16);
    let receipt = backend
        .run(Uuid::new_v4(), base_work_order(), tx)
        .await
        .unwrap();
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

// ---------------------------------------------------------------------------
// 6. Receipt timing: finished >= started, duration_ms consistent
// ---------------------------------------------------------------------------

#[tokio::test]
async fn receipt_timing_consistency() {
    let backend = MockBackend;
    let (tx, _rx) = mpsc::channel(16);
    let receipt = backend
        .run(Uuid::new_v4(), base_work_order(), tx)
        .await
        .unwrap();
    assert!(receipt.meta.finished_at >= receipt.meta.started_at);
    let delta = (receipt.meta.finished_at - receipt.meta.started_at)
        .to_std()
        .unwrap_or_default()
        .as_millis() as u64;
    assert_eq!(receipt.meta.duration_ms, delta);
}

// ---------------------------------------------------------------------------
// 7. Default execution mode is Mapped in receipt
// ---------------------------------------------------------------------------

#[tokio::test]
async fn receipt_default_execution_mode() {
    let backend = MockBackend;
    let (tx, _rx) = mpsc::channel(16);
    let receipt = backend
        .run(Uuid::new_v4(), base_work_order(), tx)
        .await
        .unwrap();
    assert_eq!(receipt.mode, ExecutionMode::Mapped);
}

// ---------------------------------------------------------------------------
// 8. Passthrough mode propagates to receipt
// ---------------------------------------------------------------------------

#[tokio::test]
async fn passthrough_mode_in_receipt() {
    let backend = MockBackend;
    let mut wo = base_work_order();
    wo.config
        .vendor
        .insert("abp".into(), json!({"mode": "passthrough"}));
    let (tx, _rx) = mpsc::channel(16);
    let receipt = backend.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert_eq!(receipt.mode, ExecutionMode::Passthrough);
}

// ---------------------------------------------------------------------------
// 9. Work order id propagated to receipt
// ---------------------------------------------------------------------------

#[tokio::test]
async fn work_order_id_propagated() {
    let backend = MockBackend;
    let wo = base_work_order();
    let expected_wo_id = wo.id;
    let (tx, _rx) = mpsc::channel(16);
    let receipt = backend.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert_eq!(receipt.meta.work_order_id, expected_wo_id);
}

// ---------------------------------------------------------------------------
// 10. Streamed event ordering: RunStarted first, RunCompleted last
// ---------------------------------------------------------------------------

#[tokio::test]
async fn event_ordering() {
    let backend = MockBackend;
    let (tx, mut rx) = mpsc::channel(32);
    let _receipt = backend
        .run(Uuid::new_v4(), base_work_order(), tx)
        .await
        .unwrap();

    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }

    assert!(events.len() >= 2, "expected at least 2 events");
    assert!(
        matches!(
            events.first().unwrap().kind,
            AgentEventKind::RunStarted { .. }
        ),
        "first event should be RunStarted"
    );
    assert!(
        matches!(
            events.last().unwrap().kind,
            AgentEventKind::RunCompleted { .. }
        ),
        "last event should be RunCompleted"
    );
}

// ---------------------------------------------------------------------------
// 11. Small channel capacity: run still completes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn small_channel_capacity() {
    let backend = MockBackend;
    let (tx, mut rx) = mpsc::channel(1);
    // Spawn a consumer to prevent blocking
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
    drop(receipt); // drop sender side implicitly done, drop receipt explicitly for clarity
    let count = consumer.await.unwrap();
    assert!(count >= 3, "expected at least 3 events, got {count}");
}

// ---------------------------------------------------------------------------
// 12. extract_execution_mode with invalid mode string falls back to default
// ---------------------------------------------------------------------------

#[test]
fn extract_execution_mode_invalid_falls_back() {
    let mut wo = base_work_order();
    wo.config
        .vendor
        .insert("abp".into(), json!({"mode": "nonexistent_mode"}));
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Mapped);
}

// ---------------------------------------------------------------------------
// 13. extract_execution_mode: nested takes priority over dotted
// ---------------------------------------------------------------------------

#[test]
fn extract_execution_mode_nested_priority() {
    let mut wo = base_work_order();
    wo.config
        .vendor
        .insert("abp".into(), json!({"mode": "passthrough"}));
    wo.config.vendor.insert("abp.mode".into(), json!("mapped"));
    // Nested "abp" object is checked first
    assert_eq!(extract_execution_mode(&wo), ExecutionMode::Passthrough);
}

// ---------------------------------------------------------------------------
// 14. validate_passthrough_compatibility with passthrough mode
// ---------------------------------------------------------------------------

#[test]
fn validate_passthrough_with_passthrough_config() {
    let mut wo = base_work_order();
    wo.config
        .vendor
        .insert("abp".into(), json!({"mode": "passthrough"}));
    assert!(validate_passthrough_compatibility(&wo).is_ok());
}

// ---------------------------------------------------------------------------
// 15. MockBackend usage_raw contains note field
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mock_backend_usage_raw() {
    let backend = MockBackend;
    let (tx, _rx) = mpsc::channel(16);
    let receipt = backend
        .run(Uuid::new_v4(), base_work_order(), tx)
        .await
        .unwrap();
    assert_eq!(receipt.usage_raw, json!({"note": "mock"}));
    assert_eq!(receipt.usage.input_tokens, Some(0));
    assert_eq!(receipt.usage.output_tokens, Some(0));
}

// ---------------------------------------------------------------------------
// 16. MockBackend verification report
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mock_backend_verification() {
    let backend = MockBackend;
    let (tx, _rx) = mpsc::channel(16);
    let receipt = backend
        .run(Uuid::new_v4(), base_work_order(), tx)
        .await
        .unwrap();
    assert!(receipt.verification.harness_ok);
    assert!(receipt.verification.git_diff.is_none());
    assert!(receipt.verification.git_status.is_none());
    assert!(receipt.artifacts.is_empty());
}

// ---------------------------------------------------------------------------
// 17. Context snippets appear in RunStarted message
// ---------------------------------------------------------------------------

#[tokio::test]
async fn run_started_contains_task() {
    let backend = MockBackend;
    let wo = WorkOrderBuilder::new("do something special").build();
    let (tx, mut rx) = mpsc::channel(16);
    let _receipt = backend.run(Uuid::new_v4(), wo, tx).await.unwrap();

    let first = rx.try_recv().unwrap();
    if let AgentEventKind::RunStarted { message } = &first.kind {
        assert!(
            message.contains("do something special"),
            "RunStarted message should contain the task"
        );
    } else {
        panic!("first event should be RunStarted");
    }
}

// ---------------------------------------------------------------------------
// 18. Unsatisfied capability requirement rejects run
// ---------------------------------------------------------------------------

#[tokio::test]
async fn unsatisfied_requirement_rejects_run() {
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
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("capability requirements not satisfied"));
}
