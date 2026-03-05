// SPDX-License-Identifier: MIT OR Apache-2.0
//! Benchmarks for dialect detection performance.

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use serde_json::{Value, json};

use abp_dialect::DialectDetector;

// ── Sample JSON builders ────────────────────────────────────────────────

fn openai_json() -> Value {
    json!({
        "model": "gpt-4",
        "messages": [
            {"role": "user", "content": "Hello"}
        ],
        "temperature": 0.7
    })
}

fn claude_json() -> Value {
    json!({
        "type": "message",
        "model": "claude-3-opus",
        "messages": [
            {"role": "user", "content": [{"type": "text", "text": "Hello"}]}
        ],
        "stop_reason": "end_turn"
    })
}

fn gemini_json() -> Value {
    json!({
        "contents": [
            {"role": "user", "parts": [{"text": "Hello"}]}
        ],
        "generationConfig": {"temperature": 0.7}
    })
}

fn codex_json() -> Value {
    json!({
        "object": "response",
        "status": "completed",
        "items": [
            {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Hi"}]}
        ]
    })
}

fn kimi_json() -> Value {
    json!({
        "model": "kimi",
        "messages": [
            {"role": "user", "content": "Hello"}
        ],
        "refs": ["ref1"],
        "search_plus": true
    })
}

fn copilot_json() -> Value {
    json!({
        "messages": [
            {"role": "user", "content": "Hello"}
        ],
        "references": [{"type": "file", "id": "src/main.rs"}],
        "agent_mode": true
    })
}

fn ambiguous_json() -> Value {
    json!({
        "model": "some-model",
        "messages": [
            {"role": "user", "content": "Hello"}
        ]
    })
}

fn large_json(n: usize) -> Value {
    let messages: Vec<Value> = (0..n)
        .map(|i| {
            json!({
                "role": if i % 2 == 0 { "user" } else { "assistant" },
                "content": format!("Message number {i} with some extra padding text to make it larger")
            })
        })
        .collect();
    json!({
        "model": "gpt-4",
        "messages": messages,
        "temperature": 0.7,
        "max_tokens": 4096
    })
}

// ── detect() per dialect ────────────────────────────────────────────────

fn bench_detect(c: &mut Criterion) {
    let mut group = c.benchmark_group("detect");
    let detector = DialectDetector::new();

    let cases = [
        ("openai", openai_json()),
        ("claude", claude_json()),
        ("gemini", gemini_json()),
        ("codex", codex_json()),
        ("kimi", kimi_json()),
        ("copilot", copilot_json()),
    ];

    for (name, json) in &cases {
        group.bench_with_input(BenchmarkId::from_parameter(name), json, |b, v| {
            b.iter(|| detector.detect(black_box(v)));
        });
    }

    group.finish();
}

// ── detect_all() with ambiguous input ───────────────────────────────────

fn bench_detect_all(c: &mut Criterion) {
    let mut group = c.benchmark_group("detect_all");
    let detector = DialectDetector::new();

    let amb = ambiguous_json();
    group.bench_function("ambiguous", |b| {
        b.iter(|| detector.detect_all(black_box(&amb)));
    });

    let oai = openai_json();
    group.bench_function("openai", |b| {
        b.iter(|| detector.detect_all(black_box(&oai)));
    });

    group.finish();
}

// ── detect() with large JSON objects ────────────────────────────────────

fn bench_detect_large(c: &mut Criterion) {
    let mut group = c.benchmark_group("detect_large");
    let detector = DialectDetector::new();

    for size in [10, 100, 500] {
        let json = large_json(size);
        group.bench_with_input(BenchmarkId::new("messages", size), &json, |b, v| {
            b.iter(|| detector.detect(black_box(v)));
        });
    }

    group.finish();
}

criterion_group!(benches, bench_detect, bench_detect_all, bench_detect_large,);
criterion_main!(benches);
