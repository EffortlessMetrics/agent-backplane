// SPDX-License-Identifier: MIT OR Apache-2.0
use criterion::{Criterion, black_box, criterion_group, criterion_main};

use abp_core::{AgentEvent, AgentEventKind};
use abp_protocol::{Envelope, JsonlCodec};
use abp_protocol::codec::StreamingCodec;
use chrono::Utc;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn event_envelope(i: usize) -> Envelope {
    Envelope::Event {
        ref_id: "run-001".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: format!("tool_{i}"),
                tool_use_id: Some(format!("tu_{i}")),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": format!("src/file_{i}.rs")}),
            },
            ext: None,
        },
    }
}

// ---------------------------------------------------------------------------
// encode_envelope — single envelope encoding
// ---------------------------------------------------------------------------

fn bench_encode_envelope(c: &mut Criterion) {
    let env = event_envelope(0);
    c.bench_function("encode_envelope", |b| {
        b.iter(|| JsonlCodec::encode(black_box(&env)).unwrap());
    });
}

// ---------------------------------------------------------------------------
// decode_envelope — single envelope decoding
// ---------------------------------------------------------------------------

fn bench_decode_envelope(c: &mut Criterion) {
    let env = event_envelope(0);
    let line = JsonlCodec::encode(&env).unwrap();
    let trimmed = line.trim().to_string();
    c.bench_function("decode_envelope", |b| {
        b.iter(|| JsonlCodec::decode(black_box(&trimmed)).unwrap());
    });
}

// ---------------------------------------------------------------------------
// batch_encode_100 — batch encode 100 envelopes
// ---------------------------------------------------------------------------

fn bench_batch_encode_100(c: &mut Criterion) {
    let envelopes: Vec<Envelope> = (0..100).map(event_envelope).collect();
    c.bench_function("batch_encode_100", |b| {
        b.iter(|| StreamingCodec::encode_batch(black_box(&envelopes)));
    });
}

// ---------------------------------------------------------------------------
// batch_decode_100 — batch decode 100 envelopes
// ---------------------------------------------------------------------------

fn bench_batch_decode_100(c: &mut Criterion) {
    let envelopes: Vec<Envelope> = (0..100).map(event_envelope).collect();
    let batch = StreamingCodec::encode_batch(&envelopes);
    c.bench_function("batch_decode_100", |b| {
        b.iter(|| {
            let results = StreamingCodec::decode_batch(black_box(&batch));
            assert_eq!(results.len(), 100);
        });
    });
}

criterion_group!(
    benches,
    bench_encode_envelope,
    bench_decode_envelope,
    bench_batch_encode_100,
    bench_batch_decode_100,
);
criterion_main!(benches);
