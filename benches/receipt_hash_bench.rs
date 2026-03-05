// SPDX-License-Identifier: MIT OR Apache-2.0
//! Benchmarks for `receipt_hash()` with varying receipt sizes.

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};

use abp_core::{AgentEvent, AgentEventKind, Outcome, ReceiptBuilder, receipt_hash};
use chrono::Utc;

/// Build a receipt whose trace contains `n` events.
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

fn bench_receipt_hash_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("receipt_hash_by_trace_size");

    for size in [0, 10, 100, 500] {
        let receipt = make_receipt(size);
        group.bench_with_input(BenchmarkId::from_parameter(size), &receipt, |b, r| {
            b.iter(|| receipt_hash(black_box(r)).unwrap());
        });
    }

    group.finish();
}

fn bench_receipt_with_hash(c: &mut Criterion) {
    let mut group = c.benchmark_group("receipt_with_hash");

    for size in [0, 50, 200] {
        let receipt = make_receipt(size);
        group.bench_with_input(BenchmarkId::from_parameter(size), &receipt, |b, r| {
            b.iter(|| black_box(r.clone()).with_hash().unwrap());
        });
    }

    group.finish();
}

fn bench_receipt_hash_determinism(c: &mut Criterion) {
    let receipt = make_receipt(50);
    c.bench_function("receipt_hash_deterministic_50_events", |b| {
        b.iter(|| {
            let h1 = receipt_hash(black_box(&receipt)).unwrap();
            let h2 = receipt_hash(black_box(&receipt)).unwrap();
            assert_eq!(h1, h2);
        });
    });
}

criterion_group!(
    benches,
    bench_receipt_hash_sizes,
    bench_receipt_with_hash,
    bench_receipt_hash_determinism,
);
criterion_main!(benches);
