// SPDX-License-Identifier: MIT OR Apache-2.0
//! Benchmark JSONL envelope parsing throughput with varying payload sizes.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::io::BufReader;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, Outcome, ReceiptBuilder,
    WorkOrderBuilder,
};
use abp_protocol::{Envelope, JsonlCodec};
use chrono::Utc;

fn small_envelope() -> String {
    let env = Envelope::hello(
        BackendIdentity {
            id: "bench".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    JsonlCodec::encode(&env).unwrap()
}

fn medium_envelope() -> String {
    let wo = WorkOrderBuilder::new("Benchmark: review this code and suggest improvements")
        .root("/tmp/workspace")
        .build();
    let env = Envelope::Run {
        id: "run-bench-001".into(),
        work_order: wo,
    };
    JsonlCodec::encode(&env).unwrap()
}

fn large_envelope(event_count: usize) -> String {
    let mut builder = ReceiptBuilder::new("bench-mock").outcome(Outcome::Complete);
    for i in 0..event_count {
        builder = builder.add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: format!("token-{i} with realistic padding content for benchmark"),
            },
            ext: None,
        });
    }
    let receipt = builder.build();
    let env = Envelope::Final {
        ref_id: "run-bench-001".into(),
        receipt,
    };
    JsonlCodec::encode(&env).unwrap()
}

fn bench_parse_single(c: &mut Criterion) {
    let mut group = c.benchmark_group("envelope_parse_single");

    let cases: Vec<(&str, String)> = vec![
        ("small_hello", small_envelope()),
        ("medium_run", medium_envelope()),
        ("large_final_50", large_envelope(50)),
        ("large_final_200", large_envelope(200)),
    ];

    for (name, json) in &cases {
        let trimmed = json.trim().to_string();
        group.throughput(Throughput::Bytes(trimmed.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(name), &trimmed, |b, j| {
            b.iter(|| JsonlCodec::decode(black_box(j)).unwrap());
        });
    }

    group.finish();
}

fn bench_parse_stream(c: &mut Criterion) {
    let mut group = c.benchmark_group("envelope_parse_stream");

    for count in [10, 100, 500] {
        let mut jsonl = String::new();
        for i in 0..count {
            let env = Envelope::Event {
                ref_id: "run-stream".into(),
                event: AgentEvent {
                    ts: Utc::now(),
                    kind: AgentEventKind::ToolCall {
                        tool_name: "read_file".into(),
                        tool_use_id: Some(format!("tu-{i}")),
                        parent_tool_use_id: None,
                        input: serde_json::json!({"path": format!("src/file_{i}.rs")}),
                    },
                    ext: None,
                },
            };
            jsonl.push_str(&JsonlCodec::encode(&env).unwrap());
        }

        group.throughput(Throughput::Bytes(jsonl.len() as u64));
        group.bench_with_input(BenchmarkId::new("events", count), &jsonl, |b, data| {
            b.iter(|| {
                let reader = BufReader::new(black_box(data.as_bytes()));
                let _: Vec<_> = JsonlCodec::decode_stream(reader)
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap();
            });
        });
    }

    group.finish();
}

fn bench_parse_mixed_stream(c: &mut Criterion) {
    let mut group = c.benchmark_group("envelope_parse_mixed");

    for count in [10, 50, 200] {
        let mut jsonl = String::new();
        // Build a realistic protocol exchange: hello + run + events + final
        jsonl.push_str(&small_envelope());
        jsonl.push_str(&medium_envelope());
        for i in 0..count {
            let env = Envelope::Event {
                ref_id: "run-mixed".into(),
                event: AgentEvent {
                    ts: Utc::now(),
                    kind: AgentEventKind::AssistantDelta {
                        text: format!("chunk-{i}"),
                    },
                    ext: None,
                },
            };
            jsonl.push_str(&JsonlCodec::encode(&env).unwrap());
        }
        jsonl.push_str(&large_envelope(0));

        group.throughput(Throughput::Bytes(jsonl.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("total_envelopes", count + 3),
            &jsonl,
            |b, data| {
                b.iter(|| {
                    let reader = BufReader::new(black_box(data.as_bytes()));
                    let _: Vec<_> = JsonlCodec::decode_stream(reader)
                        .collect::<Result<Vec<_>, _>>()
                        .unwrap();
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_parse_single,
    bench_parse_stream,
    bench_parse_mixed_stream,
);
criterion_main!(benches);
