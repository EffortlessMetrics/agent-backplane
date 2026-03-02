// SPDX-License-Identifier: MIT OR Apache-2.0
//! Benchmark receipt_hash on various receipt sizes.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};

use abp_core::{AgentEvent, AgentEventKind, Outcome, ReceiptBuilder, receipt_hash};
use abp_receipt::{ReceiptChain, canonicalize, compute_hash, verify_hash};
use chrono::Utc;

fn make_receipt(trace_len: usize) -> abp_core::Receipt {
    let mut builder = ReceiptBuilder::new("bench-backend").outcome(Outcome::Complete);
    for i in 0..trace_len {
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

fn make_receipt_with_tools(tool_pairs: usize) -> abp_core::Receipt {
    let mut builder = ReceiptBuilder::new("bench-backend").outcome(Outcome::Complete);
    for i in 0..tool_pairs {
        builder = builder.add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: format!("tool_{i}"),
                tool_use_id: Some(format!("tu-{i}")),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": format!("src/file_{i}.rs")}),
            },
            ext: None,
        });
        builder = builder.add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: format!("tool_{i}"),
                tool_use_id: Some(format!("tu-{i}")),
                output: serde_json::json!(format!("result for tool {i}")),
                is_error: false,
            },
            ext: None,
        });
    }
    builder.build()
}

fn bench_receipt_hash_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("receipt_hash_scaling");

    for size in [0, 10, 50, 200, 1000] {
        let receipt = make_receipt(size);
        let json_len = serde_json::to_string(&receipt).unwrap().len();
        group.throughput(Throughput::Bytes(json_len as u64));
        group.bench_with_input(BenchmarkId::new("events", size), &receipt, |b, r| {
            b.iter(|| receipt_hash(black_box(r)).unwrap());
        });
    }

    group.finish();
}

fn bench_canonicalize(c: &mut Criterion) {
    let mut group = c.benchmark_group("receipt_canonicalize");

    for size in [0, 50, 200, 500] {
        let receipt = make_receipt(size);
        group.bench_with_input(BenchmarkId::new("events", size), &receipt, |b, r| {
            b.iter(|| canonicalize(black_box(r)).unwrap());
        });
    }

    group.finish();
}

fn bench_compute_and_verify(c: &mut Criterion) {
    let mut group = c.benchmark_group("receipt_compute_verify");

    for size in [10, 100, 500] {
        let mut receipt = make_receipt(size);
        receipt.receipt_sha256 = Some(compute_hash(&receipt).unwrap());

        group.bench_with_input(BenchmarkId::new("compute_hash", size), &receipt, |b, r| {
            b.iter(|| compute_hash(black_box(r)).unwrap());
        });

        group.bench_with_input(BenchmarkId::new("verify_hash", size), &receipt, |b, r| {
            b.iter(|| verify_hash(black_box(r)));
        });
    }

    group.finish();
}

fn bench_receipt_hash_with_tools(c: &mut Criterion) {
    let mut group = c.benchmark_group("receipt_hash_tools");

    for tool_count in [1, 10, 50] {
        let receipt = make_receipt_with_tools(tool_count);
        group.bench_with_input(
            BenchmarkId::new("tool_pairs", tool_count),
            &receipt,
            |b, r| {
                b.iter(|| receipt_hash(black_box(r)).unwrap());
            },
        );
    }

    group.finish();
}

fn bench_receipt_chain_hashing(c: &mut Criterion) {
    let mut group = c.benchmark_group("receipt_chain_hashing");

    for chain_len in [5, 20, 50] {
        let receipts: Vec<_> = (0..chain_len)
            .map(|_| make_receipt(10).with_hash().unwrap())
            .collect();

        group.bench_with_input(
            BenchmarkId::new("chain_len", chain_len),
            &receipts,
            |b, rs| {
                b.iter(|| {
                    let mut chain = ReceiptChain::new();
                    for r in rs {
                        chain.push(black_box(r.clone())).unwrap();
                    }
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_receipt_hash_scaling,
    bench_canonicalize,
    bench_compute_and_verify,
    bench_receipt_hash_with_tools,
    bench_receipt_chain_hashing,
);
criterion_main!(benches);
