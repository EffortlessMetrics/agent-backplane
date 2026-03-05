// SPDX-License-Identifier: MIT OR Apache-2.0
//! Benchmark IR type JSON roundtrips.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition};

fn make_conversation(n: usize) -> IrConversation {
    let msgs: Vec<IrMessage> = (0..n)
        .map(|i| {
            IrMessage::new(
                if i % 2 == 0 {
                    IrRole::User
                } else {
                    IrRole::Assistant
                },
                vec![IrContentBlock::Text {
                    text: format!("Message content number {i} with some padding text for realism."),
                }],
            )
        })
        .collect();
    IrConversation::from_messages(msgs)
}

fn make_conversation_with_tools(n: usize) -> IrConversation {
    let mut msgs = Vec::new();
    for i in 0..n {
        msgs.push(IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: format!("tu-{i}"),
                name: format!("tool_{i}"),
                input: serde_json::json!({"arg": i}),
            }],
        ));
        msgs.push(IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: format!("tu-{i}"),
                content: vec![IrContentBlock::Text {
                    text: format!("result-{i}"),
                }],
                is_error: false,
            }],
        ));
    }
    IrConversation::from_messages(msgs)
}

fn make_tool_definitions(n: usize) -> Vec<IrToolDefinition> {
    (0..n)
        .map(|i| IrToolDefinition {
            name: format!("tool_{i}"),
            description: format!("A benchmark tool number {i}"),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "content": {"type": "string"}
                },
                "required": ["path"]
            }),
        })
        .collect()
}

fn bench_ir_conversation_serialize(c: &mut Criterion) {
    let mut group = c.benchmark_group("ir_conversation_serialize");

    for size in [1, 10, 50, 200] {
        let conv = make_conversation(size);
        let json = serde_json::to_string(&conv).unwrap();
        group.throughput(Throughput::Bytes(json.len() as u64));

        group.bench_with_input(BenchmarkId::new("messages", size), &conv, |b, c| {
            b.iter(|| serde_json::to_string(black_box(c)).unwrap());
        });
    }

    group.finish();
}

fn bench_ir_conversation_deserialize(c: &mut Criterion) {
    let mut group = c.benchmark_group("ir_conversation_deserialize");

    for size in [1, 10, 50, 200] {
        let conv = make_conversation(size);
        let json = serde_json::to_string(&conv).unwrap();
        group.throughput(Throughput::Bytes(json.len() as u64));

        group.bench_with_input(BenchmarkId::new("messages", size), &json, |b, j| {
            b.iter(|| serde_json::from_str::<IrConversation>(black_box(j)).unwrap());
        });
    }

    group.finish();
}

fn bench_ir_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("ir_json_roundtrip");

    for size in [1, 10, 50, 200] {
        let conv = make_conversation(size);

        group.bench_with_input(BenchmarkId::new("messages", size), &conv, |b, c| {
            b.iter(|| {
                let json = serde_json::to_string(black_box(c)).unwrap();
                serde_json::from_str::<IrConversation>(&json).unwrap()
            });
        });
    }

    group.finish();
}

fn bench_ir_tool_definitions_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("ir_tool_defs_roundtrip");

    for count in [1, 10, 50] {
        let tools = make_tool_definitions(count);

        group.bench_with_input(BenchmarkId::new("tools", count), &tools, |b, t| {
            b.iter(|| {
                let json = serde_json::to_string(black_box(t)).unwrap();
                serde_json::from_str::<Vec<IrToolDefinition>>(&json).unwrap()
            });
        });
    }

    group.finish();
}

fn bench_ir_conversation_with_tools(c: &mut Criterion) {
    let mut group = c.benchmark_group("ir_conversation_tools_roundtrip");

    for tool_count in [1, 10, 25] {
        let conv = make_conversation_with_tools(tool_count);

        group.bench_with_input(BenchmarkId::new("tool_pairs", tool_count), &conv, |b, c| {
            b.iter(|| {
                let json = serde_json::to_string(black_box(c)).unwrap();
                serde_json::from_str::<IrConversation>(&json).unwrap()
            });
        });
    }

    group.finish();
}

fn bench_ir_message_accessors(c: &mut Criterion) {
    let mut group = c.benchmark_group("ir_conversation_accessors");

    for size in [10, 50, 200] {
        let conv = make_conversation_with_tools(size);

        group.bench_with_input(BenchmarkId::new("system_message", size), &conv, |b, c| {
            b.iter(|| black_box(c).system_message());
        });

        group.bench_with_input(BenchmarkId::new("last_assistant", size), &conv, |b, c| {
            b.iter(|| black_box(c).last_assistant());
        });

        group.bench_with_input(BenchmarkId::new("tool_calls", size), &conv, |b, c| {
            b.iter(|| black_box(c).tool_calls());
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_ir_conversation_serialize,
    bench_ir_conversation_deserialize,
    bench_ir_roundtrip,
    bench_ir_tool_definitions_roundtrip,
    bench_ir_conversation_with_tools,
    bench_ir_message_accessors,
);
criterion_main!(benches);
