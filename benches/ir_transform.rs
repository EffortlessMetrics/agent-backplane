// SPDX-License-Identifier: MIT OR Apache-2.0
//! Benchmark IR transformation (SDK → IR → SDK) throughput across dialects.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

use abp_claude_sdk::dialect::ClaudeMessage;
use abp_copilot_sdk::dialect::CopilotMessage;
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition};
use abp_gemini_sdk::dialect::{GeminiContent, GeminiPart};
use abp_openai_sdk::dialect::OpenAIMessage;

// ── Builders ────────────────────────────────────────────────────────────

fn openai_messages(n: usize) -> Vec<OpenAIMessage> {
    (0..n)
        .map(|i| OpenAIMessage {
            role: if i % 2 == 0 { "user" } else { "assistant" }.into(),
            content: Some(format!(
                "Message {i} with realistic content for throughput measurement"
            )),
            tool_calls: None,
            tool_call_id: None,
        })
        .collect()
}

fn claude_messages(n: usize) -> Vec<ClaudeMessage> {
    (0..n)
        .map(|i| ClaudeMessage {
            role: if i % 2 == 0 { "user" } else { "assistant" }.into(),
            content: format!("Message {i} with realistic content for throughput measurement"),
        })
        .collect()
}

fn gemini_contents(n: usize) -> Vec<GeminiContent> {
    (0..n)
        .map(|i| GeminiContent {
            role: if i % 2 == 0 { "user" } else { "model" }.into(),
            parts: vec![GeminiPart::Text(format!(
                "Message {i} with realistic content for throughput measurement"
            ))],
        })
        .collect()
}

fn copilot_messages(n: usize) -> Vec<CopilotMessage> {
    (0..n)
        .map(|i| CopilotMessage {
            role: if i % 2 == 0 { "user" } else { "assistant" }.into(),
            content: format!("Message {i} with realistic content for throughput measurement"),
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
                    text: format!("Message {i} with realistic content for throughput measurement"),
                }],
            )
        })
        .collect();
    IrConversation::from_messages(msgs)
}

fn ir_conversation_with_tools(n: usize) -> IrConversation {
    let mut msgs = Vec::new();
    for i in 0..n {
        msgs.push(IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: format!("tu-{i}"),
                name: format!("tool_{i}"),
                input: serde_json::json!({"path": format!("src/file_{i}.rs")}),
            }],
        ));
        msgs.push(IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: format!("tu-{i}"),
                content: vec![IrContentBlock::Text {
                    text: format!("result for tool_{i}"),
                }],
                is_error: false,
            }],
        ));
    }
    IrConversation::from_messages(msgs)
}

// ── SDK → IR (small / medium / large) ───────────────────────────────────

fn bench_sdk_to_ir(c: &mut Criterion) {
    let mut group = c.benchmark_group("sdk_to_ir");

    for (label, size) in [("small", 5), ("medium", 50), ("large", 200)] {
        let oai = openai_messages(size);
        group.bench_with_input(BenchmarkId::new("openai", label), &oai, |b, msgs| {
            b.iter(|| abp_openai_sdk::lowering::to_ir(black_box(msgs)));
        });

        let cl = claude_messages(size);
        group.bench_with_input(BenchmarkId::new("claude", label), &cl, |b, msgs| {
            b.iter(|| abp_claude_sdk::lowering::to_ir(black_box(msgs), None));
        });

        let gem = gemini_contents(size);
        group.bench_with_input(BenchmarkId::new("gemini", label), &gem, |b, msgs| {
            b.iter(|| abp_gemini_sdk::lowering::to_ir(black_box(msgs), None));
        });

        let cop = copilot_messages(size);
        group.bench_with_input(BenchmarkId::new("copilot", label), &cop, |b, msgs| {
            b.iter(|| abp_copilot_sdk::lowering::to_ir(black_box(msgs)));
        });
    }

    group.finish();
}

// ── IR → SDK (small / medium / large) ───────────────────────────────────

fn bench_ir_to_sdk(c: &mut Criterion) {
    let mut group = c.benchmark_group("ir_to_sdk");

    for (label, size) in [("small", 5), ("medium", 50), ("large", 200)] {
        let conv = ir_conversation(size);

        group.bench_with_input(BenchmarkId::new("openai", label), &conv, |b, c| {
            b.iter(|| abp_openai_sdk::lowering::from_ir(black_box(c)));
        });

        group.bench_with_input(BenchmarkId::new("claude", label), &conv, |b, c| {
            b.iter(|| abp_claude_sdk::lowering::from_ir(black_box(c)));
        });

        group.bench_with_input(BenchmarkId::new("gemini", label), &conv, |b, c| {
            b.iter(|| abp_gemini_sdk::lowering::from_ir(black_box(c)));
        });

        group.bench_with_input(BenchmarkId::new("copilot", label), &conv, |b, c| {
            b.iter(|| abp_copilot_sdk::lowering::from_ir(black_box(c)));
        });
    }

    group.finish();
}

// ── Full roundtrip: SDK → IR → SDK ──────────────────────────────────────

fn bench_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("sdk_ir_sdk_roundtrip");

    for (label, size) in [("small", 5), ("medium", 50), ("large", 200)] {
        let oai = openai_messages(size);
        group.bench_with_input(BenchmarkId::new("openai", label), &oai, |b, msgs| {
            b.iter(|| {
                let conv = abp_openai_sdk::lowering::to_ir(black_box(msgs));
                abp_openai_sdk::lowering::from_ir(&conv)
            });
        });

        let cl = claude_messages(size);
        group.bench_with_input(BenchmarkId::new("claude", label), &cl, |b, msgs| {
            b.iter(|| {
                let conv = abp_claude_sdk::lowering::to_ir(black_box(msgs), None);
                abp_claude_sdk::lowering::from_ir(&conv)
            });
        });

        let gem = gemini_contents(size);
        group.bench_with_input(BenchmarkId::new("gemini", label), &gem, |b, msgs| {
            b.iter(|| {
                let conv = abp_gemini_sdk::lowering::to_ir(black_box(msgs), None);
                abp_gemini_sdk::lowering::from_ir(&conv)
            });
        });
    }

    group.finish();
}

// ── Tool-heavy roundtrip ────────────────────────────────────────────────

fn bench_tool_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("ir_tool_transform");

    for (label, tool_count) in [("small", 5), ("medium", 25), ("large", 100)] {
        let conv = ir_conversation_with_tools(tool_count);
        let json = serde_json::to_string(&conv).unwrap();
        group.throughput(Throughput::Bytes(json.len() as u64));

        group.bench_with_input(
            BenchmarkId::new("serialize_deserialize", label),
            &conv,
            |b, c| {
                b.iter(|| {
                    let json = serde_json::to_string(black_box(c)).unwrap();
                    serde_json::from_str::<IrConversation>(&json).unwrap()
                });
            },
        );
    }

    group.finish();
}

// ── Tool definitions transform ──────────────────────────────────────────

fn bench_tool_defs_transform(c: &mut Criterion) {
    let mut group = c.benchmark_group("ir_tool_defs_transform");

    for (label, count) in [("small", 5), ("medium", 25), ("large", 100)] {
        let tools: Vec<IrToolDefinition> = (0..count)
            .map(|i| IrToolDefinition {
                name: format!("tool_{i}"),
                description: format!("A benchmark tool number {i} that does something useful"),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string"},
                        "content": {"type": "string"},
                        "line": {"type": "integer"}
                    },
                    "required": ["path"]
                }),
            })
            .collect();

        let json = serde_json::to_string(&tools).unwrap();
        group.throughput(Throughput::Bytes(json.len() as u64));

        group.bench_with_input(BenchmarkId::new("roundtrip", label), &tools, |b, t| {
            b.iter(|| {
                let json = serde_json::to_string(black_box(t)).unwrap();
                serde_json::from_str::<Vec<IrToolDefinition>>(&json).unwrap()
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_sdk_to_ir,
    bench_ir_to_sdk,
    bench_roundtrip,
    bench_tool_roundtrip,
    bench_tool_defs_transform,
);
criterion_main!(benches);
