// SPDX-License-Identifier: MIT OR Apache-2.0
//! Benchmark JSONL envelope serialize/deserialize.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::io::BufReader;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, Outcome, ReceiptBuilder,
    WorkOrderBuilder,
};
use abp_protocol::{Envelope, JsonlCodec};
use chrono::Utc;

fn hello_envelope() -> Envelope {
    Envelope::hello(
        BackendIdentity {
            id: "bench-sidecar".into(),
            backend_version: Some("2.0.0".into()),
            adapter_version: Some("0.1.0".into()),
        },
        CapabilityManifest::new(),
    )
}

fn run_envelope() -> Envelope {
    let wo = WorkOrderBuilder::new("Benchmark task")
        .root("/tmp/bench")
        .build();
    Envelope::Run {
        id: "run-bench-001".into(),
        work_order: wo,
    }
}

fn event_envelope() -> Envelope {
    Envelope::Event {
        ref_id: "run-bench-001".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu-001".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "src/main.rs"}),
            },
            ext: None,
        },
    }
}

fn final_envelope() -> Envelope {
    let receipt = ReceiptBuilder::new("bench-mock")
        .outcome(Outcome::Complete)
        .build();
    Envelope::Final {
        ref_id: "run-bench-001".into(),
        receipt,
    }
}

fn fatal_envelope() -> Envelope {
    Envelope::Fatal {
        ref_id: Some("run-bench-001".into()),
        error: "out of memory".into(),
        error_code: None,
    }
}

fn bench_envelope_encode_variants(c: &mut Criterion) {
    let mut group = c.benchmark_group("envelope_encode");

    let cases: Vec<(&str, Envelope)> = vec![
        ("hello", hello_envelope()),
        ("run", run_envelope()),
        ("event", event_envelope()),
        ("final", final_envelope()),
        ("fatal", fatal_envelope()),
    ];

    for (name, env) in &cases {
        group.bench_with_input(BenchmarkId::new("variant", name), env, |b, e| {
            b.iter(|| JsonlCodec::encode(black_box(e)).unwrap());
        });
    }

    group.finish();
}

fn bench_envelope_decode_variants(c: &mut Criterion) {
    let mut group = c.benchmark_group("envelope_decode");

    let cases: Vec<(&str, String)> = vec![
        ("hello", JsonlCodec::encode(&hello_envelope()).unwrap()),
        ("run", JsonlCodec::encode(&run_envelope()).unwrap()),
        ("event", JsonlCodec::encode(&event_envelope()).unwrap()),
        ("final", JsonlCodec::encode(&final_envelope()).unwrap()),
        ("fatal", JsonlCodec::encode(&fatal_envelope()).unwrap()),
    ];

    for (name, json) in &cases {
        let trimmed = json.trim().to_string();
        group.bench_with_input(BenchmarkId::new("variant", name), &trimmed, |b, j| {
            b.iter(|| JsonlCodec::decode(black_box(j)).unwrap());
        });
    }

    group.finish();
}

fn bench_envelope_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("envelope_roundtrip");

    let cases: Vec<(&str, Envelope)> = vec![
        ("hello", hello_envelope()),
        ("run", run_envelope()),
        ("event", event_envelope()),
        ("final", final_envelope()),
    ];

    for (name, env) in &cases {
        group.bench_with_input(BenchmarkId::new("variant", name), env, |b, e| {
            b.iter(|| {
                let s = JsonlCodec::encode(black_box(e)).unwrap();
                JsonlCodec::decode(s.trim()).unwrap()
            });
        });
    }

    group.finish();
}

fn bench_decode_stream(c: &mut Criterion) {
    let mut group = c.benchmark_group("envelope_decode_stream");

    for count in [10, 100, 500] {
        let mut jsonl = String::new();
        for i in 0..count {
            let env = Envelope::Event {
                ref_id: "run-stream".into(),
                event: AgentEvent {
                    ts: Utc::now(),
                    kind: AgentEventKind::AssistantDelta {
                        text: format!("token-{i}"),
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

fn bench_encode_to_writer(c: &mut Criterion) {
    let mut group = c.benchmark_group("envelope_encode_writer");

    for count in [10, 100, 500] {
        let envelopes: Vec<Envelope> = (0..count)
            .map(|i| Envelope::Event {
                ref_id: "run-writer".into(),
                event: AgentEvent {
                    ts: Utc::now(),
                    kind: AgentEventKind::AssistantDelta {
                        text: format!("chunk-{i}"),
                    },
                    ext: None,
                },
            })
            .collect();

        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(
            BenchmarkId::new("envelopes", count),
            &envelopes,
            |b, envs| {
                b.iter(|| {
                    let mut buf = Vec::with_capacity(4096);
                    JsonlCodec::encode_many_to_writer(&mut buf, black_box(envs)).unwrap();
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_envelope_encode_variants,
    bench_envelope_decode_variants,
    bench_envelope_roundtrip,
    bench_decode_stream,
    bench_encode_to_writer,
);
criterion_main!(benches);
