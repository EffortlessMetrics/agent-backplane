// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive benchmarks for receipt construction, hashing, chain
//! operations, and canonical JSON serialization.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use std::collections::BTreeMap;

use abp_core::chain::ReceiptChain;
use abp_core::{
    AgentEvent, AgentEventKind, Outcome, Receipt, ReceiptBuilder, canonical_json, receipt_hash,
    sha256_hex,
};
use chrono::Utc;

// ── Helpers ─────────────────────────────────────────────────────────────

fn make_receipt(trace_len: usize) -> Receipt {
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

fn make_receipt_with_tools(tool_count: usize) -> Receipt {
    let mut builder = ReceiptBuilder::new("bench-backend").outcome(Outcome::Complete);
    for i in 0..tool_count {
        builder = builder.add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: format!("tool_{i}"),
                tool_use_id: Some(format!("tu-{i:04}")),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": format!("src/file_{i}.rs")}),
            },
            ext: None,
        });
        builder = builder.add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: format!("tool_{i}"),
                tool_use_id: Some(format!("tu-{i:04}")),
                output: serde_json::json!(format!("result for tool_{i}")),
                is_error: false,
            },
            ext: None,
        });
    }
    builder.build()
}

// ── Receipt construction benchmarks ─────────────────────────────────────

fn bench_receipt_builder(c: &mut Criterion) {
    let mut group = c.benchmark_group("receipt_builder");

    for size in [0, 10, 50, 200] {
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::new("trace_events", size), &size, |b, &n| {
            b.iter(|| make_receipt(black_box(n)));
        });
    }

    group.finish();
}

fn bench_receipt_builder_with_tools(c: &mut Criterion) {
    let mut group = c.benchmark_group("receipt_builder_tools");

    for count in [1, 10, 50] {
        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(BenchmarkId::new("tool_pairs", count), &count, |b, &n| {
            b.iter(|| make_receipt_with_tools(black_box(n)));
        });
    }

    group.finish();
}

// ── Canonical JSON benchmarks ───────────────────────────────────────────

fn bench_canonical_json(c: &mut Criterion) {
    let mut group = c.benchmark_group("canonical_json");

    for size in [0, 10, 100, 500] {
        let receipt = make_receipt(size);
        let json_len = serde_json::to_string(&receipt).unwrap().len();
        group.throughput(Throughput::Bytes(json_len as u64));
        group.bench_with_input(BenchmarkId::new("receipt", size), &receipt, |b, r| {
            b.iter(|| canonical_json(black_box(r)).unwrap());
        });
    }

    // Nested BTreeMap — canonical_json's key-sorted guarantee
    let nested: BTreeMap<String, BTreeMap<String, Vec<i32>>> = (0..50)
        .map(|i| {
            let inner: BTreeMap<String, Vec<i32>> = (0..10)
                .map(|j| (format!("key_{j}"), (0..20).collect()))
                .collect();
            (format!("outer_{i}"), inner)
        })
        .collect();
    let nested_len = serde_json::to_string(&nested).unwrap().len();
    group.throughput(Throughput::Bytes(nested_len as u64));
    group.bench_function("nested_btreemap", |b| {
        b.iter(|| canonical_json(black_box(&nested)).unwrap());
    });

    group.finish();
}

// ── SHA-256 throughput ──────────────────────────────────────────────────

fn bench_sha256_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("sha256_throughput");

    for &size in &[64, 1024, 16384, 65536] {
        let data: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::new("bytes", size), &data, |b, d| {
            b.iter(|| sha256_hex(black_box(d)));
        });
    }

    group.finish();
}

// ── Receipt hash benchmarks (expanded) ──────────────────────────────────

fn bench_receipt_hash(c: &mut Criterion) {
    let mut group = c.benchmark_group("receipt_hash");

    for size in [0, 10, 50, 200, 1000] {
        let receipt = make_receipt(size);
        let json_len = serde_json::to_string(&receipt).unwrap().len();
        group.throughput(Throughput::Bytes(json_len as u64));
        group.bench_with_input(BenchmarkId::new("trace_size", size), &receipt, |b, r| {
            b.iter(|| receipt_hash(black_box(r)).unwrap());
        });
    }

    // with_hash includes cloning + hashing
    for size in [0, 50, 200] {
        let receipt = make_receipt(size);
        group.bench_with_input(BenchmarkId::new("with_hash", size), &receipt, |b, r| {
            b.iter(|| black_box(r.clone()).with_hash().unwrap());
        });
    }

    group.finish();
}

// ── Receipt chain benchmarks ────────────────────────────────────────────

fn bench_chain_push(c: &mut Criterion) {
    let mut group = c.benchmark_group("receipt_chain_push");

    for chain_len in [1, 10, 50] {
        let receipts: Vec<Receipt> = (0..chain_len)
            .map(|_| {
                ReceiptBuilder::new("bench")
                    .outcome(Outcome::Complete)
                    .with_hash()
                    .unwrap()
            })
            .collect();

        group.bench_with_input(BenchmarkId::new("length", chain_len), &receipts, |b, rs| {
            b.iter(|| {
                let mut chain = ReceiptChain::new();
                for r in rs {
                    chain.push(black_box(r.clone())).unwrap();
                }
                chain
            });
        });
    }

    group.finish();
}

fn bench_chain_verify(c: &mut Criterion) {
    let mut group = c.benchmark_group("receipt_chain_verify");

    for chain_len in [5, 20, 50] {
        let mut chain = ReceiptChain::new();
        for _ in 0..chain_len {
            let receipt = ReceiptBuilder::new("bench")
                .outcome(Outcome::Complete)
                .with_hash()
                .unwrap();
            chain.push(receipt).unwrap();
        }

        group.bench_with_input(BenchmarkId::new("length", chain_len), &chain, |b, c| {
            b.iter(|| black_box(c).verify().unwrap());
        });
    }

    group.finish();
}

fn bench_chain_analytics(c: &mut Criterion) {
    let mut chain = ReceiptChain::new();
    for _ in 0..20 {
        let receipt = make_receipt(10).with_hash().unwrap();
        chain.push(receipt).unwrap();
    }

    c.bench_function("chain_success_rate", |b| {
        b.iter(|| black_box(&chain).success_rate());
    });

    c.bench_function("chain_total_events", |b| {
        b.iter(|| black_box(&chain).total_events());
    });

    c.bench_function("chain_duration_range", |b| {
        b.iter(|| black_box(&chain).duration_range());
    });
}

criterion_group!(
    benches,
    bench_receipt_builder,
    bench_receipt_builder_with_tools,
    bench_canonical_json,
    bench_sha256_throughput,
    bench_receipt_hash,
    bench_chain_push,
    bench_chain_verify,
    bench_chain_analytics,
);
criterion_main!(benches);
