// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive benchmarks for envelope parse/serialize, JSONL stream
//! processing, batch operations, validation, and routing.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use std::io::BufReader;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, Outcome, ReceiptBuilder,
    WorkOrderBuilder,
};
use abp_protocol::batch::{BatchProcessor, BatchRequest};
use abp_protocol::router::{MessageRoute, MessageRouter};
use abp_protocol::stream::StreamParser;
use abp_protocol::validate::EnvelopeValidator;
use abp_protocol::{Envelope, JsonlCodec};
use chrono::Utc;

// ── Helpers ─────────────────────────────────────────────────────────────

fn hello_envelope() -> Envelope {
    Envelope::hello(
        BackendIdentity {
            id: "bench-sidecar".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    )
}

fn run_envelope() -> Envelope {
    let wo = WorkOrderBuilder::new("Benchmark protocol task")
        .root("/tmp/bench")
        .model("gpt-4")
        .build();
    Envelope::Run {
        id: "run-bench-001".into(),
        work_order: wo,
    }
}

fn event_envelope(i: usize) -> Envelope {
    Envelope::Event {
        ref_id: "run-bench-001".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: format!("token-{i}"),
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
        ref_id: "run-bench-001".into(),
        receipt,
    }
}

fn fatal_envelope() -> Envelope {
    Envelope::Fatal {
        ref_id: Some("run-bench-001".into()),
        error: "benchmark error".into(),
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

// ── Envelope encode/decode per variant ──────────────────────────────────

fn bench_envelope_encode(c: &mut Criterion) {
    let mut group = c.benchmark_group("envelope_encode");

    let variants: Vec<(&str, Envelope)> = vec![
        ("hello", hello_envelope()),
        ("run", run_envelope()),
        ("event", event_envelope(0)),
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

    group.finish();
}

fn bench_envelope_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("envelope_decode");

    let variants: Vec<(&str, Envelope)> = vec![
        ("hello", hello_envelope()),
        ("run", run_envelope()),
        ("event", event_envelope(0)),
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

    group.finish();
}

// ── JSONL stream decode (BufRead) ───────────────────────────────────────

fn bench_decode_stream(c: &mut Criterion) {
    let mut group = c.benchmark_group("jsonl_decode_stream");

    for event_count in [10, 100, 500] {
        let data = build_jsonl_stream(event_count);
        // +2 for hello + run, +1 for final
        let total_envelopes = event_count + 3;
        group.throughput(Throughput::Elements(total_envelopes as u64));

        group.bench_with_input(BenchmarkId::new("events", event_count), &data, |b, d| {
            b.iter(|| {
                let reader = BufReader::new(black_box(d.as_slice()));
                let count = JsonlCodec::decode_stream(reader)
                    .filter(|r| r.is_ok())
                    .count();
                assert_eq!(count, total_envelopes);
            });
        });
    }

    group.finish();
}

// ── Incremental stream parser ───────────────────────────────────────────

fn bench_stream_parser(c: &mut Criterion) {
    let mut group = c.benchmark_group("stream_parser");

    for event_count in [10, 100, 500] {
        let data = build_jsonl_stream(event_count);
        let total_envelopes = event_count + 3;
        group.throughput(Throughput::Elements(total_envelopes as u64));

        // Feed entire buffer at once
        group.bench_with_input(
            BenchmarkId::new("whole_buffer", event_count),
            &data,
            |b, d| {
                b.iter(|| {
                    let mut parser = StreamParser::new();
                    let results = parser.push(black_box(d));
                    assert_eq!(results.len(), total_envelopes);
                });
            },
        );

        // Feed in small chunks (simulating async I/O)
        group.bench_with_input(
            BenchmarkId::new("chunked_128b", event_count),
            &data,
            |b, d| {
                b.iter(|| {
                    let mut parser = StreamParser::new();
                    let mut count = 0;
                    for chunk in d.chunks(128) {
                        count += parser.push(black_box(chunk)).len();
                    }
                    count += parser.finish().len();
                    assert_eq!(count, total_envelopes);
                });
            },
        );
    }

    group.finish();
}

// ── Envelope validation ─────────────────────────────────────────────────

fn bench_validate_envelope(c: &mut Criterion) {
    let mut group = c.benchmark_group("envelope_validate");
    let validator = EnvelopeValidator::new();

    let variants: Vec<(&str, Envelope)> = vec![
        ("hello", hello_envelope()),
        ("run", run_envelope()),
        ("event", event_envelope(0)),
        ("final", final_envelope()),
        ("fatal", fatal_envelope()),
    ];

    for (name, env) in &variants {
        group.bench_with_input(BenchmarkId::from_parameter(name), env, |b, e| {
            b.iter(|| validator.validate(black_box(e)));
        });
    }

    group.finish();
}

fn bench_validate_sequence(c: &mut Criterion) {
    let mut group = c.benchmark_group("sequence_validate");
    let validator = EnvelopeValidator::new();

    for event_count in [5, 20, 100] {
        let mut seq = vec![hello_envelope(), run_envelope()];
        for i in 0..event_count {
            seq.push(event_envelope(i));
        }
        seq.push(final_envelope());

        group.bench_with_input(BenchmarkId::new("events", event_count), &seq, |b, s| {
            b.iter(|| validator.validate_sequence(black_box(s)));
        });
    }

    group.finish();
}

// ── Batch processing ────────────────────────────────────────────────────

fn bench_batch_process(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_process");
    let processor = BatchProcessor::new();

    for batch_size in [10, 100, 500] {
        let envelopes: Vec<Envelope> = (0..batch_size).map(event_envelope).collect();
        let request = BatchRequest {
            id: "batch-bench".into(),
            envelopes,
            created_at: Utc::now().to_rfc3339(),
        };

        group.throughput(Throughput::Elements(batch_size as u64));
        group.bench_with_input(BenchmarkId::new("items", batch_size), &request, |b, r| {
            b.iter(|| processor.process(black_box(r.clone())));
        });
    }

    group.finish();
}

fn bench_batch_validate(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_validate");
    let processor = BatchProcessor::new();

    for batch_size in [10, 100, 500] {
        let envelopes: Vec<Envelope> = (0..batch_size).map(event_envelope).collect();
        let request = BatchRequest {
            id: "batch-bench".into(),
            envelopes,
            created_at: Utc::now().to_rfc3339(),
        };

        group.throughput(Throughput::Elements(batch_size as u64));
        group.bench_with_input(BenchmarkId::new("items", batch_size), &request, |b, r| {
            b.iter(|| processor.validate_batch(black_box(r)));
        });
    }

    group.finish();
}

// ── Message routing ─────────────────────────────────────────────────────

fn bench_message_routing(c: &mut Criterion) {
    let mut group = c.benchmark_group("message_routing");

    for route_count in [5, 20, 50] {
        let mut router = MessageRouter::new();
        for i in 0..route_count {
            router.add_route(MessageRoute {
                pattern: format!("ref-{i}"),
                destination: format!("handler-{i}"),
                priority: i as u32,
            });
        }
        // Add type-based routes
        for t in &["hello", "run", "event", "final", "fatal"] {
            router.add_route(MessageRoute {
                pattern: (*t).into(),
                destination: format!("{t}-handler"),
                priority: 100,
            });
        }

        let env = event_envelope(0);
        group.bench_with_input(
            BenchmarkId::new("route_single", route_count),
            &(&router, &env),
            |b, (r, e)| {
                b.iter(|| r.route(black_box(e)));
            },
        );

        let envelopes: Vec<Envelope> = (0..50).map(event_envelope).collect();
        group.bench_with_input(
            BenchmarkId::new("route_all_50", route_count),
            &(&router, &envelopes),
            |b, (r, es)| {
                b.iter(|| r.route_all(black_box(es)));
            },
        );
    }

    group.finish();
}

// ── Writer throughput ───────────────────────────────────────────────────

fn bench_encode_to_writer(c: &mut Criterion) {
    let mut group = c.benchmark_group("encode_to_writer");

    for count in [10, 100, 500] {
        let envelopes: Vec<Envelope> = (0..count).map(event_envelope).collect();
        let total_bytes: usize = envelopes
            .iter()
            .map(|e| JsonlCodec::encode(e).unwrap().len())
            .sum();
        group.throughput(Throughput::Bytes(total_bytes as u64));

        group.bench_with_input(BenchmarkId::new("envelopes", count), &envelopes, |b, es| {
            b.iter(|| {
                let mut buf = Vec::with_capacity(total_bytes);
                JsonlCodec::encode_many_to_writer(&mut buf, black_box(es)).unwrap();
                buf
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_envelope_encode,
    bench_envelope_decode,
    bench_decode_stream,
    bench_stream_parser,
    bench_validate_envelope,
    bench_validate_sequence,
    bench_batch_process,
    bench_batch_validate,
    bench_message_routing,
    bench_encode_to_writer,
);
criterion_main!(benches);
