// SPDX-License-Identifier: MIT OR Apache-2.0
//! Benchmark receipt hashing throughput with small, medium, and large receipts.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};

use abp_core::{AgentEvent, AgentEventKind, Outcome, ReceiptBuilder, receipt_hash};
use chrono::Utc;

/// Build a receipt whose trace contains `n` events with realistic payloads.
fn make_receipt(trace_len: usize) -> abp_core::Receipt {
    let mut builder = ReceiptBuilder::new("bench-backend").outcome(Outcome::Complete);
    for i in 0..trace_len {
        let kind = match i % 3 {
            0 => AgentEventKind::AssistantDelta {
                text: format!("token-{i} with some realistic padding content for benchmarking"),
            },
            1 => AgentEventKind::ToolCall {
                tool_name: format!("tool_{i}"),
                tool_use_id: Some(format!("tu-{i}")),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": format!("src/file_{i}.rs"), "content": "x".repeat(50)}),
            },
            _ => AgentEventKind::ToolResult {
                tool_name: format!("tool_{i}"),
                tool_use_id: Some(format!("tu-{i}")),
                output: serde_json::json!(format!("result-{i}: operation completed successfully")),
                is_error: false,
            },
        };
        builder = builder.add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind,
            ext: None,
        });
    }
    builder.build()
}

fn bench_hash_small(c: &mut Criterion) {
    let mut group = c.benchmark_group("receipt_hash_small");
    for size in [0, 1, 5] {
        let receipt = make_receipt(size);
        let json = serde_json::to_string(&receipt).unwrap();
        group.throughput(Throughput::Bytes(json.len() as u64));
        group.bench_with_input(BenchmarkId::new("events", size), &receipt, |b, r| {
            b.iter(|| receipt_hash(black_box(r)).unwrap());
        });
    }
    group.finish();
}

fn bench_hash_medium(c: &mut Criterion) {
    let mut group = c.benchmark_group("receipt_hash_medium");
    for size in [10, 50, 100] {
        let receipt = make_receipt(size);
        let json = serde_json::to_string(&receipt).unwrap();
        group.throughput(Throughput::Bytes(json.len() as u64));
        group.bench_with_input(BenchmarkId::new("events", size), &receipt, |b, r| {
            b.iter(|| receipt_hash(black_box(r)).unwrap());
        });
    }
    group.finish();
}

fn bench_hash_large(c: &mut Criterion) {
    let mut group = c.benchmark_group("receipt_hash_large");
    for size in [500, 1000] {
        let receipt = make_receipt(size);
        let json = serde_json::to_string(&receipt).unwrap();
        group.throughput(Throughput::Bytes(json.len() as u64));
        group.bench_with_input(BenchmarkId::new("events", size), &receipt, |b, r| {
            b.iter(|| receipt_hash(black_box(r)).unwrap());
        });
    }
    group.finish();
}

fn bench_with_hash(c: &mut Criterion) {
    let mut group = c.benchmark_group("receipt_with_hash_sizes");
    for (label, size) in [("small", 5), ("medium", 50), ("large", 500)] {
        let receipt = make_receipt(size);
        group.bench_with_input(BenchmarkId::from_parameter(label), &receipt, |b, r| {
            b.iter(|| black_box(r.clone()).with_hash().unwrap());
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_hash_small,
    bench_hash_medium,
    bench_hash_large,
    bench_with_hash,
);
criterion_main!(benches);
