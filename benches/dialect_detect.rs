// SPDX-License-Identifier: MIT OR Apache-2.0
//! Benchmark dialect detection speed with varying JSON sizes and dialects.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use serde_json::{json, Value};

use abp_dialect::DialectDetector;

// ── Sample JSON builders ────────────────────────────────────────────────

fn openai_small() -> Value {
    json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "Hi"}]
    })
}

fn openai_medium(n: usize) -> Value {
    let messages: Vec<Value> = (0..n)
        .map(|i| json!({"role": if i % 2 == 0 { "user" } else { "assistant" }, "content": format!("Message {i}")}))
        .collect();
    json!({"model": "gpt-4", "messages": messages, "temperature": 0.7, "max_tokens": 4096})
}

fn openai_large(n: usize) -> Value {
    let messages: Vec<Value> = (0..n)
        .map(|i| json!({
            "role": if i % 2 == 0 { "user" } else { "assistant" },
            "content": format!("Message {i} with extensive content for large payload benchmarking purposes")
        }))
        .collect();
    json!({"model": "gpt-4", "messages": messages, "temperature": 0.7, "max_tokens": 4096, "tools": []})
}

fn claude_small() -> Value {
    json!({
        "type": "message",
        "model": "claude-3-opus",
        "messages": [{"role": "user", "content": [{"type": "text", "text": "Hi"}]}]
    })
}

fn gemini_small() -> Value {
    json!({
        "contents": [{"role": "user", "parts": [{"text": "Hi"}]}],
        "generationConfig": {"temperature": 0.7}
    })
}

fn codex_small() -> Value {
    json!({
        "object": "response",
        "status": "completed",
        "items": [{"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Hi"}]}]
    })
}

fn kimi_small() -> Value {
    json!({
        "model": "kimi",
        "messages": [{"role": "user", "content": "Hi"}],
        "refs": ["ref1"],
        "search_plus": true
    })
}

fn copilot_small() -> Value {
    json!({
        "messages": [{"role": "user", "content": "Hi"}],
        "references": [{"type": "file", "id": "src/main.rs"}],
        "agent_mode": true
    })
}

fn ambiguous_json() -> Value {
    json!({
        "model": "some-model",
        "messages": [{"role": "user", "content": "Hello"}]
    })
}

// ── Per-dialect detection speed ─────────────────────────────────────────

fn bench_detect_per_dialect(c: &mut Criterion) {
    let mut group = c.benchmark_group("detect_per_dialect");
    let detector = DialectDetector::new();

    let cases: Vec<(&str, Value)> = vec![
        ("openai", openai_small()),
        ("claude", claude_small()),
        ("gemini", gemini_small()),
        ("codex", codex_small()),
        ("kimi", kimi_small()),
        ("copilot", copilot_small()),
        ("ambiguous", ambiguous_json()),
    ];

    for (name, val) in &cases {
        group.bench_with_input(BenchmarkId::from_parameter(name), val, |b, v| {
            b.iter(|| detector.detect(black_box(v)));
        });
    }

    group.finish();
}

// ── Detection by input size ─────────────────────────────────────────────

fn bench_detect_by_size(c: &mut Criterion) {
    let mut group = c.benchmark_group("detect_by_size");
    let detector = DialectDetector::new();

    for (label, val) in [
        ("small", openai_small()),
        ("medium_50", openai_medium(50)),
        ("medium_100", openai_medium(100)),
        ("large_500", openai_large(500)),
    ] {
        let json_str = serde_json::to_string(&val).unwrap();
        group.throughput(Throughput::Bytes(json_str.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(label), &val, |b, v| {
            b.iter(|| detector.detect(black_box(v)));
        });
    }

    group.finish();
}

// ── detect_all() throughput ─────────────────────────────────────────────

fn bench_detect_all(c: &mut Criterion) {
    let mut group = c.benchmark_group("detect_all");
    let detector = DialectDetector::new();

    let cases: Vec<(&str, Value)> = vec![
        ("openai", openai_small()),
        ("ambiguous", ambiguous_json()),
        ("claude", claude_small()),
        ("gemini", gemini_small()),
    ];

    for (name, val) in &cases {
        group.bench_with_input(BenchmarkId::from_parameter(name), val, |b, v| {
            b.iter(|| detector.detect_all(black_box(v)));
        });
    }

    group.finish();
}

// ── Batch detection throughput ──────────────────────────────────────────

fn bench_detect_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("detect_batch");
    let detector = DialectDetector::new();

    let payloads: Vec<Value> = vec![
        openai_small(),
        claude_small(),
        gemini_small(),
        codex_small(),
        kimi_small(),
        copilot_small(),
        ambiguous_json(),
        openai_medium(20),
        openai_large(50),
    ];

    group.throughput(Throughput::Elements(payloads.len() as u64));
    group.bench_function("mixed_payloads", |b| {
        b.iter(|| {
            for p in &payloads {
                let _ = detector.detect(black_box(p));
            }
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_detect_per_dialect,
    bench_detect_by_size,
    bench_detect_all,
    bench_detect_batch,
);
criterion_main!(benches);
