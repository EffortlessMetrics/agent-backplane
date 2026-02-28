// SPDX-License-Identifier: MIT OR Apache-2.0
use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION,
    ExecutionLane, ExecutionMode, Outcome, PolicyProfile,
    Receipt, RunMetadata, UsageNormalized, VerificationReport,
    WorkOrder, WorkOrderBuilder, canonical_json, receipt_hash,
};
use chrono::Utc;
use std::collections::BTreeMap;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_event(i: usize) -> AgentEvent {
    let now = Utc::now();
    match i % 4 {
        0 => AgentEvent {
            ts: now,
            kind: AgentEventKind::AssistantDelta {
                text: format!("token {i}"),
            },
            ext: None,
        },
        1 => AgentEvent {
            ts: now,
            kind: AgentEventKind::ToolCall {
                tool_name: "Read".into(),
                tool_use_id: Some(format!("tu_{i}")),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": format!("src/file_{i}.rs")}),
            },
            ext: None,
        },
        2 => AgentEvent {
            ts: now,
            kind: AgentEventKind::ToolResult {
                tool_name: "Read".into(),
                tool_use_id: Some(format!("tu_{i}")),
                output: serde_json::json!({"content": "fn main() {}"}),
                is_error: false,
            },
            ext: None,
        },
        _ => AgentEvent {
            ts: now,
            kind: AgentEventKind::FileChanged {
                path: format!("src/file_{i}.rs"),
                summary: "added function".into(),
            },
            ext: None,
        },
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
// Receipt hashing: small / medium / large
// ---------------------------------------------------------------------------

fn bench_receipt_hash_sizes(c: &mut Criterion) {
    let small = make_receipt(0);
    let medium = make_receipt(10);
    let large = make_receipt(200);

    let mut group = c.benchmark_group("receipt_hash");
    group.bench_function("small_0_events", |b| {
        b.iter(|| receipt_hash(black_box(&small)).unwrap());
    });
    group.bench_function("medium_10_events", |b| {
        b.iter(|| receipt_hash(black_box(&medium)).unwrap());
    });
    group.bench_function("large_200_events", |b| {
        b.iter(|| receipt_hash(black_box(&large)).unwrap());
    });
    group.finish();
}

// ---------------------------------------------------------------------------
// WorkOrder serialization / deserialization
// ---------------------------------------------------------------------------

fn bench_work_order_serde(c: &mut Criterion) {
    let wo = make_work_order();
    let json = serde_json::to_string(&wo).unwrap();

    let mut group = c.benchmark_group("work_order");
    group.bench_function("serialize", |b| {
        b.iter(|| serde_json::to_string(black_box(&wo)).unwrap());
    });
    group.bench_function("deserialize", |b| {
        b.iter(|| serde_json::from_str::<WorkOrder>(black_box(&json)).unwrap());
    });
    group.finish();
}

// ---------------------------------------------------------------------------
// AgentEvent serialization
// ---------------------------------------------------------------------------

fn bench_agent_event_serialize(c: &mut Criterion) {
    let events: Vec<AgentEvent> = (0..4).map(make_event).collect();

    let mut group = c.benchmark_group("agent_event_serialize");
    let labels = ["assistant_delta", "tool_call", "tool_result", "file_changed"];
    for (event, label) in events.iter().zip(labels.iter()) {
        group.bench_with_input(BenchmarkId::from_parameter(label), event, |b, ev| {
            b.iter(|| serde_json::to_string(black_box(ev)).unwrap());
        });
    }
    group.finish();
}

// ---------------------------------------------------------------------------
// Canonical JSON
// ---------------------------------------------------------------------------

fn bench_canonical_json(c: &mut Criterion) {
    let wo = make_work_order();
    let small_receipt = make_receipt(0);
    let large_receipt = make_receipt(200);

    let mut group = c.benchmark_group("canonical_json");
    group.bench_function("work_order", |b| {
        b.iter(|| canonical_json(black_box(&wo)).unwrap());
    });
    group.bench_function("receipt_small", |b| {
        b.iter(|| canonical_json(black_box(&small_receipt)).unwrap());
    });
    group.bench_function("receipt_large", |b| {
        b.iter(|| canonical_json(black_box(&large_receipt)).unwrap());
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_receipt_hash_sizes,
    bench_work_order_serde,
    bench_agent_event_serialize,
    bench_canonical_json,
);
criterion_main!(benches);
