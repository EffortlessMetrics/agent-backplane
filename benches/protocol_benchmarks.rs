// SPDX-License-Identifier: MIT OR Apache-2.0
//! Protocol-layer benchmarks: JSONL envelope serialization, parsing,
//! roundtrip, and large work-order serialization.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::io::BufReader;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, Outcome, ReceiptBuilder,
    WorkOrderBuilder,
};
use abp_protocol::stream::StreamParser;
use abp_protocol::{Envelope, JsonlCodec};
use chrono::Utc;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn hello_envelope() -> Envelope {
    Envelope::hello(
        BackendIdentity {
            id: "bench-sidecar".into(),
            backend_version: Some("2.0.0".into()),
            adapter_version: Some("0.5.0".into()),
        },
        CapabilityManifest::new(),
    )
}

fn run_envelope() -> Envelope {
    let wo = WorkOrderBuilder::new("Benchmark protocol task")
        .root("/tmp/bench")
        .model("gpt-4o")
        .max_turns(10)
        .build();
    Envelope::Run {
        id: "run-proto-001".into(),
        work_order: wo,
    }
}

fn event_envelope(i: usize) -> Envelope {
    Envelope::Event {
        ref_id: "run-proto-001".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: format!("token-{i}"),
            },
            ext: None,
        },
    }
}

fn tool_event_envelope(i: usize) -> Envelope {
    Envelope::Event {
        ref_id: "run-proto-001".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: format!("read_file_{i}"),
                tool_use_id: Some(format!("tu-{i:04}")),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": format!("src/mod_{i}.rs")}),
            },
            ext: None,
        },
    }
}

fn final_envelope() -> Envelope {
    let receipt = ReceiptBuilder::new("bench")
        .outcome(Outcome::Complete)
        .build();
    Envelope::Final {
        ref_id: "run-proto-001".into(),
        receipt,
    }
}

fn fatal_envelope() -> Envelope {
    Envelope::Fatal {
        ref_id: Some("run-proto-001".into()),
        error: "benchmark fatal error".into(),
        error_code: None,
    }
}

fn build_jsonl_stream(event_count: usize) -> Vec<u8> {
    let mut lines = Vec::new();
    lines.push(JsonlCodec::encode(&hello_envelope()).unwrap());
    lines.push(JsonlCodec::encode(&run_envelope()).unwrap());
    for i in 0..event_count {
        lines.push(JsonlCodec::encode(&event_envelope(i)).unwrap());
    }
    lines.push(JsonlCodec::encode(&final_envelope()).unwrap());
    lines.concat().into_bytes()
}

fn large_work_order() -> Envelope {
    let mut builder = WorkOrderBuilder::new(
        "Perform a comprehensive security audit of the entire authentication \
         subsystem, including OAuth2 flows, JWT token validation, session \
         management, CSRF protection, and rate limiting. Identify all \
         potential vulnerabilities and provide detailed remediation steps.",
    )
    .root("/workspace/large-project")
    .model("gpt-4o")
    .max_turns(50)
    .max_budget_usd(10.0);

    let include: Vec<String> = (0..50).map(|i| format!("src/auth/mod_{i}/**")).collect();
    let exclude: Vec<String> = (0..20).map(|i| format!("vendor_{i}/**")).collect();
    builder = builder.include(include).exclude(exclude);

    Envelope::Run {
        id: "run-large-001".into(),
        work_order: builder.build(),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. JSONL envelope serialization
// ═══════════════════════════════════════════════════════════════════════════

fn bench_envelope_serialize(c: &mut Criterion) {
    let mut group = c.benchmark_group("protocol/envelope_serialize");

    let variants: Vec<(&str, Envelope)> = vec![
        ("hello", hello_envelope()),
        ("run", run_envelope()),
        ("event_delta", event_envelope(0)),
        ("event_tool", tool_event_envelope(0)),
        ("final", final_envelope()),
        ("fatal", fatal_envelope()),
    ];

    for (name, env) in &variants {
        let encoded = JsonlCodec::encode(env).unwrap();
        group.throughput(Throughput::Bytes(encoded.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(name), env, |b, e| {
            b.iter(|| JsonlCodec::encode(black_box(e)).unwrap());
        });
    }

    // Batch: serialize 50 envelopes
    let batch: Vec<Envelope> = (0..50).map(event_envelope).collect();
    let total_bytes: usize = batch
        .iter()
        .map(|e| JsonlCodec::encode(e).unwrap().len())
        .sum();
    group.throughput(Throughput::Bytes(total_bytes as u64));
    group.bench_function("batch_50_events", |b| {
        b.iter(|| {
            for e in black_box(&batch) {
                black_box(JsonlCodec::encode(e).unwrap());
            }
        });
    });

    group.finish();
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. JSONL envelope parsing
// ═══════════════════════════════════════════════════════════════════════════

fn bench_envelope_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("protocol/envelope_parse");

    let variants: Vec<(&str, Envelope)> = vec![
        ("hello", hello_envelope()),
        ("run", run_envelope()),
        ("event_delta", event_envelope(0)),
        ("event_tool", tool_event_envelope(0)),
        ("final", final_envelope()),
        ("fatal", fatal_envelope()),
    ];

    for (name, env) in &variants {
        let encoded = JsonlCodec::encode(env).unwrap();
        let trimmed = encoded.trim().to_string();
        group.throughput(Throughput::Bytes(trimmed.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(name), &trimmed, |b, s| {
            b.iter(|| JsonlCodec::decode(black_box(s)).unwrap());
        });
    }

    // Batch: parse 50 pre-encoded envelopes
    let encoded_batch: Vec<String> = (0..50)
        .map(|i| {
            let e = event_envelope(i);
            JsonlCodec::encode(&e).unwrap().trim().to_string()
        })
        .collect();
    let total_bytes: usize = encoded_batch.iter().map(|s| s.len()).sum();
    group.throughput(Throughput::Bytes(total_bytes as u64));
    group.bench_function("batch_50_events", |b| {
        b.iter(|| {
            for s in black_box(&encoded_batch) {
                black_box(JsonlCodec::decode(s).unwrap());
            }
        });
    });

    group.finish();
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Envelope roundtrip (encode → decode, 100 events)
// ═══════════════════════════════════════════════════════════════════════════

fn bench_envelope_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("protocol/envelope_roundtrip");

    // Baseline: 10 events
    for event_count in [10, 100] {
        let envelopes: Vec<Envelope> = (0..event_count).map(event_envelope).collect();
        let total_bytes: usize = envelopes
            .iter()
            .map(|e| JsonlCodec::encode(e).unwrap().len())
            .sum();
        group.throughput(Throughput::Elements(event_count as u64));

        group.bench_with_input(
            BenchmarkId::new("encode_decode", event_count),
            &envelopes,
            |b, es| {
                b.iter(|| {
                    for e in black_box(es) {
                        let s = JsonlCodec::encode(e).unwrap();
                        let trimmed = s.trim();
                        black_box(JsonlCodec::decode(trimmed).unwrap());
                    }
                });
            },
        );

        // Full stream roundtrip: build JSONL → parse via StreamParser
        let stream_data = build_jsonl_stream(event_count);
        group.throughput(Throughput::Bytes(total_bytes as u64));
        group.bench_with_input(
            BenchmarkId::new("stream_roundtrip", event_count),
            &stream_data,
            |b, d| {
                b.iter(|| {
                    let mut parser = StreamParser::new();
                    let results = parser.push(black_box(d));
                    black_box(results.len());
                });
            },
        );
    }

    // BufReader-based decode_stream
    for event_count in [10, 100] {
        let data = build_jsonl_stream(event_count);
        let total_envelopes = event_count + 3;
        group.throughput(Throughput::Elements(total_envelopes as u64));

        group.bench_with_input(
            BenchmarkId::new("bufreader_decode_stream", event_count),
            &data,
            |b, d| {
                b.iter(|| {
                    let reader = BufReader::new(black_box(d.as_slice()));
                    let count = JsonlCodec::decode_stream(reader)
                        .filter(|r| r.is_ok())
                        .count();
                    assert_eq!(count, total_envelopes);
                });
            },
        );
    }

    group.finish();
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Large work order serialization
// ═══════════════════════════════════════════════════════════════════════════

fn bench_large_work_order(c: &mut Criterion) {
    let mut group = c.benchmark_group("protocol/large_work_order");

    let large_env = large_work_order();
    let encoded = JsonlCodec::encode(&large_env).unwrap();
    let trimmed = encoded.trim().to_string();

    // Serialize
    group.throughput(Throughput::Bytes(encoded.len() as u64));
    group.bench_function("serialize", |b| {
        b.iter(|| JsonlCodec::encode(black_box(&large_env)).unwrap());
    });

    // Deserialize
    group.throughput(Throughput::Bytes(trimmed.len() as u64));
    group.bench_function("deserialize", |b| {
        b.iter(|| JsonlCodec::decode(black_box(&trimmed)).unwrap());
    });

    // Roundtrip
    group.bench_function("roundtrip", |b| {
        b.iter(|| {
            let s = JsonlCodec::encode(black_box(&large_env)).unwrap();
            let t = s.trim();
            black_box(JsonlCodec::decode(t).unwrap());
        });
    });

    // Compare: serde_json direct (without JSONL framing)
    if let Envelope::Run { ref work_order, .. } = large_env {
        let wo_json = serde_json::to_string(work_order).unwrap();
        group.throughput(Throughput::Bytes(wo_json.len() as u64));
        group.bench_function("serde_json_direct_roundtrip", |b| {
            b.iter(|| {
                let s = serde_json::to_string(black_box(work_order)).unwrap();
                serde_json::from_str::<abp_core::WorkOrder>(&s).unwrap()
            });
        });
    }

    group.finish();
}

// ═══════════════════════════════════════════════════════════════════════════
// Criterion groups & main
// ═══════════════════════════════════════════════════════════════════════════

criterion_group!(
    benches,
    bench_envelope_serialize,
    bench_envelope_parse,
    bench_envelope_roundtrip,
    bench_large_work_order,
);
criterion_main!(benches);
