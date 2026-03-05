// SPDX-License-Identifier: MIT OR Apache-2.0
//! Benchmarks for JSON serialization/deserialization of core contract types.

use criterion::{Criterion, black_box, criterion_group, criterion_main};

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, Outcome, ReceiptBuilder,
    WorkOrderBuilder,
};
use abp_protocol::{Envelope, JsonlCodec};
use chrono::Utc;

fn sample_work_order() -> abp_core::WorkOrder {
    WorkOrderBuilder::new("Benchmark task: refactor the authentication module")
        .root("/tmp/bench-workspace")
        .model("gpt-4")
        .max_turns(20)
        .build()
}

fn sample_receipt() -> abp_core::Receipt {
    let mut builder = ReceiptBuilder::new("bench-mock").outcome(Outcome::Complete);
    for i in 0..20 {
        builder = builder.add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: format!("token-{i}"),
            },
            ext: None,
        });
    }
    builder.build()
}

fn sample_envelope() -> Envelope {
    Envelope::hello(
        BackendIdentity {
            id: "bench-sidecar".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    )
}

fn sample_agent_event() -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu-001".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"path": "src/main.rs"}),
        },
        ext: None,
    }
}

fn bench_work_order_roundtrip(c: &mut Criterion) {
    let wo = sample_work_order();
    let json = serde_json::to_string(&wo).unwrap();

    c.bench_function("work_order_serialize", |b| {
        b.iter(|| serde_json::to_string(black_box(&wo)).unwrap());
    });

    c.bench_function("work_order_deserialize", |b| {
        b.iter(|| serde_json::from_str::<abp_core::WorkOrder>(black_box(&json)).unwrap());
    });

    c.bench_function("work_order_roundtrip", |b| {
        b.iter(|| {
            let s = serde_json::to_string(black_box(&wo)).unwrap();
            let _: abp_core::WorkOrder = serde_json::from_str(&s).unwrap();
        });
    });
}

fn bench_receipt_roundtrip(c: &mut Criterion) {
    let receipt = sample_receipt();
    let json = serde_json::to_string(&receipt).unwrap();

    c.bench_function("receipt_serialize", |b| {
        b.iter(|| serde_json::to_string(black_box(&receipt)).unwrap());
    });

    c.bench_function("receipt_deserialize", |b| {
        b.iter(|| serde_json::from_str::<abp_core::Receipt>(black_box(&json)).unwrap());
    });

    c.bench_function("receipt_roundtrip", |b| {
        b.iter(|| {
            let s = serde_json::to_string(black_box(&receipt)).unwrap();
            let _: abp_core::Receipt = serde_json::from_str(&s).unwrap();
        });
    });
}

fn bench_envelope_roundtrip(c: &mut Criterion) {
    let env = sample_envelope();
    let encoded = JsonlCodec::encode(&env).unwrap();

    c.bench_function("envelope_encode", |b| {
        b.iter(|| JsonlCodec::encode(black_box(&env)).unwrap());
    });

    c.bench_function("envelope_decode", |b| {
        b.iter(|| JsonlCodec::decode(black_box(encoded.trim())).unwrap());
    });

    c.bench_function("envelope_roundtrip", |b| {
        b.iter(|| {
            let s = JsonlCodec::encode(black_box(&env)).unwrap();
            let _ = JsonlCodec::decode(s.trim()).unwrap();
        });
    });
}

fn bench_agent_event_roundtrip(c: &mut Criterion) {
    let event = sample_agent_event();
    let json = serde_json::to_string(&event).unwrap();

    c.bench_function("agent_event_serialize", |b| {
        b.iter(|| serde_json::to_string(black_box(&event)).unwrap());
    });

    c.bench_function("agent_event_deserialize", |b| {
        b.iter(|| serde_json::from_str::<AgentEvent>(black_box(&json)).unwrap());
    });

    c.bench_function("agent_event_roundtrip", |b| {
        b.iter(|| {
            let s = serde_json::to_string(black_box(&event)).unwrap();
            let _: AgentEvent = serde_json::from_str(&s).unwrap();
        });
    });
}

criterion_group!(
    benches,
    bench_work_order_roundtrip,
    bench_receipt_roundtrip,
    bench_envelope_roundtrip,
    bench_agent_event_roundtrip,
);
criterion_main!(benches);
