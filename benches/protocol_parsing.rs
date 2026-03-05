// SPDX-License-Identifier: MIT OR Apache-2.0
//! Benchmark JSONL envelope parsing and serialization throughput.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::io::BufReader;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, Outcome, ReceiptBuilder,
    WorkOrderBuilder,
};
use abp_protocol::{Envelope, JsonlCodec};
use chrono::Utc;

// ── Envelope factories ──────────────────────────────────────────────────

fn hello_envelope() -> Envelope {
    Envelope::hello(
        BackendIdentity {
            id: "bench-sidecar".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: Some("0.3.0".into()),
        },
        CapabilityManifest::new(),
    )
}

fn run_envelope() -> Envelope {
    let wo = WorkOrderBuilder::new("Review this code and suggest improvements")
        .root("/tmp/workspace")
        .model("gpt-4")
        .max_turns(25)
        .build();
    Envelope::Run {
        id: "run-001".into(),
        work_order: wo,
    }
}

fn event_envelope_delta(text: &str) -> Envelope {
    Envelope::Event {
        ref_id: "run-001".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: text.to_string(),
            },
            ext: None,
        },
    }
}

fn event_envelope_tool_call(i: usize) -> Envelope {
    Envelope::Event {
        ref_id: "run-001".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some(format!("tu-{i}")),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": format!("src/module_{i}/lib.rs")}),
            },
            ext: None,
        },
    }
}

fn final_envelope(trace_len: usize) -> Envelope {
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
    Envelope::Final {
        ref_id: "run-001".into(),
        receipt: builder.build(),
    }
}

fn fatal_envelope() -> Envelope {
    Envelope::Fatal {
        ref_id: Some("run-001".into()),
        error: "Rate limit exceeded after 3 retries".into(),
        error_code: None,
    }
}

// ── Single-line decode by variant ───────────────────────────────────────

fn bench_decode_by_variant(c: &mut Criterion) {
    let mut group = c.benchmark_group("protocol_parse/decode_variant");

    let variants: Vec<(&str, String)> = vec![
        ("hello", JsonlCodec::encode(&hello_envelope()).unwrap()),
        ("run", JsonlCodec::encode(&run_envelope()).unwrap()),
        (
            "event_delta",
            JsonlCodec::encode(&event_envelope_delta("Hello world")).unwrap(),
        ),
        (
            "event_tool",
            JsonlCodec::encode(&event_envelope_tool_call(0)).unwrap(),
        ),
        ("final_10", JsonlCodec::encode(&final_envelope(10)).unwrap()),
        ("fatal", JsonlCodec::encode(&fatal_envelope()).unwrap()),
    ];

    for (name, json) in &variants {
        let trimmed = json.trim();
        group.throughput(Throughput::Bytes(trimmed.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(name), trimmed, |b, j| {
            b.iter(|| JsonlCodec::decode(black_box(j)).unwrap());
        });
    }

    group.finish();
}

// ── Encode throughput by variant ────────────────────────────────────────

fn bench_encode_by_variant(c: &mut Criterion) {
    let mut group = c.benchmark_group("protocol_parse/encode_variant");

    let variants: Vec<(&str, Envelope)> = vec![
        ("hello", hello_envelope()),
        ("run", run_envelope()),
        ("event_delta", event_envelope_delta("Hello world")),
        ("event_tool", event_envelope_tool_call(0)),
        ("final_10", final_envelope(10)),
        ("fatal", fatal_envelope()),
    ];

    for (name, env) in &variants {
        let json = JsonlCodec::encode(env).unwrap();
        group.throughput(Throughput::Bytes(json.trim().len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(name), env, |b, e| {
            b.iter(|| JsonlCodec::encode(black_box(e)).unwrap());
        });
    }

    group.finish();
}

// ── Decode scaling with receipt size ────────────────────────────────────

fn bench_decode_final_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("protocol_parse/final_scaling");

    for trace_len in [0, 10, 50, 200, 500] {
        let json = JsonlCodec::encode(&final_envelope(trace_len)).unwrap();
        let trimmed = json.trim().to_string();
        group.throughput(Throughput::Bytes(trimmed.len() as u64));

        group.bench_with_input(
            BenchmarkId::new("trace_events", trace_len),
            &trimmed,
            |b, j| {
                b.iter(|| JsonlCodec::decode(black_box(j)).unwrap());
            },
        );
    }

    group.finish();
}

// ── Stream parsing throughput ───────────────────────────────────────────

fn bench_stream_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("protocol_parse/stream_decode");

    for count in [10, 50, 200, 1000] {
        let mut jsonl = String::new();
        for i in 0..count {
            let env = if i % 3 == 0 {
                event_envelope_tool_call(i)
            } else {
                event_envelope_delta(&format!("token-{i}"))
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

// ── Full protocol exchange ──────────────────────────────────────────────

fn bench_full_exchange(c: &mut Criterion) {
    let mut group = c.benchmark_group("protocol_parse/full_exchange");

    for event_count in [10, 50, 200] {
        let mut jsonl = String::new();
        // hello
        jsonl.push_str(&JsonlCodec::encode(&hello_envelope()).unwrap());
        // run
        jsonl.push_str(&JsonlCodec::encode(&run_envelope()).unwrap());
        // streaming events
        for i in 0..event_count {
            jsonl.push_str(
                &JsonlCodec::encode(&event_envelope_delta(&format!("chunk-{i}"))).unwrap(),
            );
        }
        // final
        jsonl.push_str(&JsonlCodec::encode(&final_envelope(0)).unwrap());

        let total_envelopes = event_count + 3;
        group.throughput(Throughput::Elements(total_envelopes as u64));

        group.bench_with_input(
            BenchmarkId::new("envelopes", total_envelopes),
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

// ── Raw serde_json vs JsonlCodec ────────────────────────────────────────

fn bench_raw_vs_codec(c: &mut Criterion) {
    let mut group = c.benchmark_group("protocol_parse/raw_vs_codec");

    let env = run_envelope();
    let json = serde_json::to_string(&env).unwrap();

    group.throughput(Throughput::Bytes(json.len() as u64));

    group.bench_function("serde_json_from_str", |b| {
        b.iter(|| serde_json::from_str::<Envelope>(black_box(&json)).unwrap());
    });

    let codec_json = JsonlCodec::encode(&env).unwrap();
    let codec_trimmed = codec_json.trim().to_string();

    group.bench_function("jsonl_codec_decode", |b| {
        b.iter(|| JsonlCodec::decode(black_box(&codec_trimmed)).unwrap());
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_decode_by_variant,
    bench_encode_by_variant,
    bench_decode_final_scaling,
    bench_stream_decode,
    bench_full_exchange,
    bench_raw_vs_codec,
);
criterion_main!(benches);
