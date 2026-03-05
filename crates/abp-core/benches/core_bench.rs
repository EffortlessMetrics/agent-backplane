// SPDX-License-Identifier: MIT OR Apache-2.0
use criterion::{Criterion, black_box, criterion_group, criterion_main};

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, ExecutionLane,
    ExecutionMode, Outcome, PolicyProfile, Receipt, ReceiptBuilder, RunMetadata, UsageNormalized,
    VerificationReport, WorkOrder, WorkOrderBuilder, receipt_hash,
};
use chrono::Utc;
use std::collections::BTreeMap;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_event(i: usize) -> AgentEvent {
    let now = Utc::now();
    AgentEvent {
        ts: now,
        kind: AgentEventKind::ToolCall {
            tool_name: format!("tool_{i}"),
            tool_use_id: Some(format!("tu_{i}")),
            parent_tool_use_id: None,
            input: serde_json::json!({"path": format!("src/file_{i}.rs")}),
        },
        ext: None,
    }
}

fn make_receipt(trace_len: usize) -> Receipt {
    let now = Utc::now();
    Receipt {
        meta: RunMetadata {
            run_id: Uuid::nil(),
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.into(),
            started_at: now,
            finished_at: now,
            duration_ms: 1234,
        },
        backend: BackendIdentity {
            id: "mock".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: Some("0.1.0".into()),
        },
        capabilities: BTreeMap::new(),
        mode: ExecutionMode::Mapped,
        usage_raw: serde_json::json!({"tokens": 500}),
        usage: UsageNormalized {
            input_tokens: Some(200),
            output_tokens: Some(300),
            ..UsageNormalized::default()
        },
        trace: (0..trace_len).map(make_event).collect(),
        artifacts: vec![ArtifactRef {
            kind: "patch".into(),
            path: "output.patch".into(),
        }],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

fn make_work_order() -> WorkOrder {
    WorkOrderBuilder::new("Refactor the authentication module")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/home/user/project")
        .include(vec!["src/**".into(), "tests/**".into()])
        .exclude(vec!["target/**".into(), "*.log".into()])
        .model("claude-sonnet")
        .max_turns(10)
        .policy(PolicyProfile {
            allowed_tools: vec!["Read".into(), "Write".into(), "Bash".into()],
            disallowed_tools: vec!["DeleteFile".into()],
            deny_read: vec!["**/.env".into()],
            deny_write: vec!["**/locked/**".into()],
            ..PolicyProfile::default()
        })
        .build()
}

// ---------------------------------------------------------------------------
// receipt_hash — hash computation for various receipt sizes
// ---------------------------------------------------------------------------

fn bench_receipt_hash(c: &mut Criterion) {
    let small = make_receipt(0);
    let medium = make_receipt(50);
    let large = make_receipt(500);

    let mut group = c.benchmark_group("receipt_hash");
    group.bench_function("0_events", |b| {
        b.iter(|| receipt_hash(black_box(&small)).unwrap());
    });
    group.bench_function("50_events", |b| {
        b.iter(|| receipt_hash(black_box(&medium)).unwrap());
    });
    group.bench_function("500_events", |b| {
        b.iter(|| receipt_hash(black_box(&large)).unwrap());
    });
    group.finish();
}

// ---------------------------------------------------------------------------
// work_order_serialize — JSON serialization of WorkOrder
// ---------------------------------------------------------------------------

fn bench_work_order_serialize(c: &mut Criterion) {
    let wo = make_work_order();
    c.bench_function("work_order_serialize", |b| {
        b.iter(|| serde_json::to_string(black_box(&wo)).unwrap());
    });
}

// ---------------------------------------------------------------------------
// work_order_deserialize — JSON deserialization of WorkOrder
// ---------------------------------------------------------------------------

fn bench_work_order_deserialize(c: &mut Criterion) {
    let wo = make_work_order();
    let json = serde_json::to_string(&wo).unwrap();
    c.bench_function("work_order_deserialize", |b| {
        b.iter(|| serde_json::from_str::<WorkOrder>(black_box(&json)).unwrap());
    });
}

// ---------------------------------------------------------------------------
// receipt_builder — ReceiptBuilder construction
// ---------------------------------------------------------------------------

fn bench_receipt_builder(c: &mut Criterion) {
    let events: Vec<AgentEvent> = (0..20).map(make_event).collect();

    c.bench_function("receipt_builder", |b| {
        b.iter(|| {
            let mut builder = ReceiptBuilder::new("mock")
                .outcome(Outcome::Complete)
                .backend_version("1.0.0")
                .adapter_version("0.1.0")
                .mode(ExecutionMode::Mapped);
            for event in &events {
                builder = builder.add_trace_event(event.clone());
            }
            black_box(builder.build())
        });
    });
}

criterion_group!(
    benches,
    bench_receipt_hash,
    bench_work_order_serialize,
    bench_work_order_deserialize,
    bench_receipt_builder,
);
criterion_main!(benches);
