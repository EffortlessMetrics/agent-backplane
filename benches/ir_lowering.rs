// SPDX-License-Identifier: MIT OR Apache-2.0
//! Benchmarks for IR lowering performance across all 6 SDK dialects.

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};

use abp_claude_sdk::dialect::ClaudeMessage;
use abp_codex_sdk::dialect::CodexResponseItem;
use abp_copilot_sdk::dialect::CopilotMessage;
use abp_gemini_sdk::dialect::{GeminiContent, GeminiPart};
use abp_kimi_sdk::dialect::KimiMessage;
use abp_openai_sdk::dialect::OpenAIMessage;

// ── Sample builders ─────────────────────────────────────────────────────

fn openai_messages(n: usize) -> Vec<OpenAIMessage> {
    (0..n)
        .map(|i| OpenAIMessage {
            role: if i % 2 == 0 { "user" } else { "assistant" }.into(),
            content: Some(format!("Message {i}")),
            tool_calls: None,
            tool_call_id: None,
        })
        .collect()
}

fn claude_messages(n: usize) -> Vec<ClaudeMessage> {
    (0..n)
        .map(|i| ClaudeMessage {
            role: if i % 2 == 0 { "user" } else { "assistant" }.into(),
            content: format!("Message {i}"),
        })
        .collect()
}

fn gemini_contents(n: usize) -> Vec<GeminiContent> {
    (0..n)
        .map(|i| GeminiContent {
            role: if i % 2 == 0 { "user" } else { "model" }.into(),
            parts: vec![GeminiPart::Text(format!("Message {i}"))],
        })
        .collect()
}

fn codex_items(n: usize) -> Vec<CodexResponseItem> {
    (0..n)
        .map(|i| CodexResponseItem::Message {
            role: "assistant".into(),
            content: vec![abp_codex_sdk::dialect::CodexContentPart::OutputText {
                text: format!("Message {i}"),
            }],
        })
        .collect()
}

fn kimi_messages(n: usize) -> Vec<KimiMessage> {
    (0..n)
        .map(|i| KimiMessage {
            role: if i % 2 == 0 { "user" } else { "assistant" }.into(),
            content: Some(format!("Message {i}")),
            tool_call_id: None,
            tool_calls: None,
        })
        .collect()
}

fn copilot_messages(n: usize) -> Vec<CopilotMessage> {
    (0..n)
        .map(|i| CopilotMessage {
            role: if i % 2 == 0 { "user" } else { "assistant" }.into(),
            content: format!("Message {i}"),
            name: None,
            copilot_references: Vec::new(),
        })
        .collect()
}

fn ir_conversation(n: usize) -> IrConversation {
    let msgs: Vec<IrMessage> = (0..n)
        .map(|i| {
            IrMessage::new(
                if i % 2 == 0 {
                    IrRole::User
                } else {
                    IrRole::Assistant
                },
                vec![IrContentBlock::Text {
                    text: format!("Message {i}"),
                }],
            )
        })
        .collect();
    IrConversation::from_messages(msgs)
}

// ── to_ir benchmarks ────────────────────────────────────────────────────

fn bench_to_ir(c: &mut Criterion) {
    let mut group = c.benchmark_group("to_ir");

    for size in [1, 10, 50] {
        let oai = openai_messages(size);
        group.bench_with_input(BenchmarkId::new("openai", size), &oai, |b, msgs| {
            b.iter(|| abp_openai_sdk::lowering::to_ir(black_box(msgs)));
        });

        let cl = claude_messages(size);
        group.bench_with_input(BenchmarkId::new("claude", size), &cl, |b, msgs| {
            b.iter(|| abp_claude_sdk::lowering::to_ir(black_box(msgs), None));
        });

        let gem = gemini_contents(size);
        group.bench_with_input(BenchmarkId::new("gemini", size), &gem, |b, msgs| {
            b.iter(|| abp_gemini_sdk::lowering::to_ir(black_box(msgs), None));
        });

        let cdx = codex_items(size);
        group.bench_with_input(BenchmarkId::new("codex", size), &cdx, |b, items| {
            b.iter(|| abp_codex_sdk::lowering::to_ir(black_box(items)));
        });

        let kimi = kimi_messages(size);
        group.bench_with_input(BenchmarkId::new("kimi", size), &kimi, |b, msgs| {
            b.iter(|| abp_kimi_sdk::lowering::to_ir(black_box(msgs)));
        });

        let cop = copilot_messages(size);
        group.bench_with_input(BenchmarkId::new("copilot", size), &cop, |b, msgs| {
            b.iter(|| abp_copilot_sdk::lowering::to_ir(black_box(msgs)));
        });
    }

    group.finish();
}

// ── from_ir benchmarks ──────────────────────────────────────────────────

fn bench_from_ir(c: &mut Criterion) {
    let mut group = c.benchmark_group("from_ir");

    for size in [1, 10, 50] {
        let conv = ir_conversation(size);

        group.bench_with_input(BenchmarkId::new("openai", size), &conv, |b, c| {
            b.iter(|| abp_openai_sdk::lowering::from_ir(black_box(c)));
        });

        group.bench_with_input(BenchmarkId::new("claude", size), &conv, |b, c| {
            b.iter(|| abp_claude_sdk::lowering::from_ir(black_box(c)));
        });

        group.bench_with_input(BenchmarkId::new("gemini", size), &conv, |b, c| {
            b.iter(|| abp_gemini_sdk::lowering::from_ir(black_box(c)));
        });

        group.bench_with_input(BenchmarkId::new("codex", size), &conv, |b, c| {
            b.iter(|| abp_codex_sdk::lowering::from_ir(black_box(c)));
        });

        group.bench_with_input(BenchmarkId::new("kimi", size), &conv, |b, c| {
            b.iter(|| abp_kimi_sdk::lowering::from_ir(black_box(c)));
        });

        group.bench_with_input(BenchmarkId::new("copilot", size), &conv, |b, c| {
            b.iter(|| abp_copilot_sdk::lowering::from_ir(black_box(c)));
        });
    }

    group.finish();
}

// ── roundtrip benchmarks ────────────────────────────────────────────────

fn bench_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("ir_roundtrip");

    for size in [1, 10, 50] {
        let oai = openai_messages(size);
        group.bench_with_input(BenchmarkId::new("openai", size), &oai, |b, msgs| {
            b.iter(|| {
                let conv = abp_openai_sdk::lowering::to_ir(black_box(msgs));
                abp_openai_sdk::lowering::from_ir(&conv)
            });
        });

        let cl = claude_messages(size);
        group.bench_with_input(BenchmarkId::new("claude", size), &cl, |b, msgs| {
            b.iter(|| {
                let conv = abp_claude_sdk::lowering::to_ir(black_box(msgs), None);
                abp_claude_sdk::lowering::from_ir(&conv)
            });
        });

        let gem = gemini_contents(size);
        group.bench_with_input(BenchmarkId::new("gemini", size), &gem, |b, msgs| {
            b.iter(|| {
                let conv = abp_gemini_sdk::lowering::to_ir(black_box(msgs), None);
                abp_gemini_sdk::lowering::from_ir(&conv)
            });
        });

        let cdx = codex_items(size);
        group.bench_with_input(BenchmarkId::new("codex", size), &cdx, |b, items| {
            b.iter(|| {
                let conv = abp_codex_sdk::lowering::to_ir(black_box(items));
                abp_codex_sdk::lowering::from_ir(&conv)
            });
        });

        let kimi = kimi_messages(size);
        group.bench_with_input(BenchmarkId::new("kimi", size), &kimi, |b, msgs| {
            b.iter(|| {
                let conv = abp_kimi_sdk::lowering::to_ir(black_box(msgs));
                abp_kimi_sdk::lowering::from_ir(&conv)
            });
        });

        let cop = copilot_messages(size);
        group.bench_with_input(BenchmarkId::new("copilot", size), &cop, |b, msgs| {
            b.iter(|| {
                let conv = abp_copilot_sdk::lowering::to_ir(black_box(msgs));
                abp_copilot_sdk::lowering::from_ir(&conv)
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_to_ir, bench_from_ir, bench_roundtrip,);
criterion_main!(benches);
