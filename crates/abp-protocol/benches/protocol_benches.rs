// SPDX-License-Identifier: MIT OR Apache-2.0
use criterion::{Criterion, black_box, criterion_group, criterion_main};

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, Outcome,
    ReceiptBuilder, WorkOrderBuilder,
};
use abp_protocol::{Envelope, JsonlCodec, is_compatible_version, parse_version};
use chrono::Utc;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn hello_envelope() -> Envelope {
    Envelope::hello(
        BackendIdentity {
            id: "bench-sidecar".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: Some("0.2.0".into()),
        },
        CapabilityManifest::new(),
    )
}

fn event_envelope() -> Envelope {
    Envelope::Event {
        ref_id: "run-001".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "Read".into(),
                tool_use_id: Some("tu_1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "src/lib.rs"}),
            },
            ext: None,
        },
    }
}

fn run_envelope() -> Envelope {
    Envelope::Run {
        id: "run-001".into(),
        work_order: WorkOrderBuilder::new("bench task").build(),
    }
}

fn final_envelope() -> Envelope {
    Envelope::Final {
        ref_id: "run-001".into(),
        receipt: ReceiptBuilder::new("mock").outcome(Outcome::Complete).build(),
    }
}

fn fatal_envelope() -> Envelope {
    Envelope::Fatal {
        ref_id: Some("run-001".into()),
        error: "something went wrong".into(),
    }
}

// ---------------------------------------------------------------------------
// Envelope serialization / deserialization
// ---------------------------------------------------------------------------

fn bench_envelope_serde(c: &mut Criterion) {
    let envelopes = [
        ("hello", hello_envelope()),
        ("event", event_envelope()),
        ("run", run_envelope()),
        ("final", final_envelope()),
        ("fatal", fatal_envelope()),
    ];

    let mut group = c.benchmark_group("envelope_serialize");
    for (name, env) in &envelopes {
        group.bench_function(*name, |b| {
            b.iter(|| JsonlCodec::encode(black_box(env)).unwrap());
        });
    }
    group.finish();

    let encoded: Vec<(&str, String)> = envelopes
        .iter()
        .map(|(n, e)| (*n, JsonlCodec::encode(e).unwrap()))
        .collect();

    let mut group = c.benchmark_group("envelope_deserialize");
    for (name, line) in &encoded {
        group.bench_function(*name, |b| {
            b.iter(|| JsonlCodec::decode(black_box(line.trim())).unwrap());
        });
    }
    group.finish();
}

// ---------------------------------------------------------------------------
// JSONL parsing: single line, batch of 100
// ---------------------------------------------------------------------------

fn bench_jsonl_parsing(c: &mut Criterion) {
    let single_line = JsonlCodec::encode(&event_envelope()).unwrap();

    let batch: String = (0..100)
        .map(|_| JsonlCodec::encode(&event_envelope()).unwrap())
        .collect();

    let mut group = c.benchmark_group("jsonl_parse");
    group.bench_function("single_line", |b| {
        b.iter(|| JsonlCodec::decode(black_box(single_line.trim())).unwrap());
    });
    group.bench_function("batch_100_lines", |b| {
        b.iter(|| {
            let reader = std::io::BufReader::new(black_box(batch.as_bytes()));
            let count = JsonlCodec::decode_stream(reader)
                .filter(|r| r.is_ok())
                .count();
            assert_eq!(count, 100);
        });
    });
    group.finish();
}

// ---------------------------------------------------------------------------
// Version string parsing
// ---------------------------------------------------------------------------

fn bench_version_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("version");
    group.bench_function("parse_valid", |b| {
        b.iter(|| parse_version(black_box("abp/v0.1")));
    });
    group.bench_function("parse_invalid", |b| {
        b.iter(|| parse_version(black_box("not-a-version")));
    });
    group.bench_function("is_compatible_same_major", |b| {
        b.iter(|| is_compatible_version(black_box("abp/v0.1"), black_box("abp/v0.2")));
    });
    group.bench_function("is_compatible_diff_major", |b| {
        b.iter(|| is_compatible_version(black_box("abp/v1.0"), black_box("abp/v0.1")));
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_envelope_serde,
    bench_jsonl_parsing,
    bench_version_parsing,
);
criterion_main!(benches);
