// SPDX-License-Identifier: MIT OR Apache-2.0
//! Benchmarks for projection matrix lookups and message mapping.

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};

use abp_core::WorkOrderBuilder;
use abp_integrations::projection::{
    Dialect, Message, MessageRole, ProjectionMatrix, ToolCall, ToolDefinitionIr,
};

fn bench_matrix_construction(c: &mut Criterion) {
    c.bench_function("projection_matrix_new", |b| {
        b.iter(|| black_box(ProjectionMatrix::new()));
    });
}

fn bench_work_order_translate(c: &mut Criterion) {
    let mut group = c.benchmark_group("wo_translate");
    let matrix = ProjectionMatrix::new();
    let wo = WorkOrderBuilder::new("Benchmark translation task")
        .root("/tmp/bench")
        .model("gpt-4")
        .build();

    let targets = [
        ("identity", Dialect::Abp, Dialect::Abp),
        ("abp_to_claude", Dialect::Abp, Dialect::Claude),
        ("abp_to_openai", Dialect::Abp, Dialect::OpenAi),
        ("abp_to_gemini", Dialect::Abp, Dialect::Gemini),
        ("abp_to_codex", Dialect::Abp, Dialect::Codex),
        ("abp_to_mock", Dialect::Abp, Dialect::Mock),
    ];

    for (name, from, to) in &targets {
        group.bench_with_input(
            BenchmarkId::new("pair", name),
            &(&matrix, &wo),
            |b, (m, w)| {
                b.iter(|| {
                    m.translate(black_box(*from), black_box(*to), black_box(w))
                        .unwrap()
                });
            },
        );
    }

    group.finish();
}

fn bench_tool_call_translate(c: &mut Criterion) {
    let mut group = c.benchmark_group("tool_call_translate");
    let matrix = ProjectionMatrix::new();

    let call = ToolCall {
        tool_name: "read_file".into(),
        tool_use_id: Some("tu-001".into()),
        parent_tool_use_id: None,
        input: serde_json::json!({"path": "src/main.rs"}),
    };

    let pairs = [
        ("abp_to_openai", "abp", "openai"),
        ("abp_to_anthropic", "abp", "anthropic"),
        ("abp_to_gemini", "abp", "gemini"),
        ("identity", "abp", "abp"),
    ];

    for (name, from, to) in &pairs {
        group.bench_with_input(
            BenchmarkId::new("pair", name),
            &(&matrix, &call),
            |b, (m, tc)| {
                b.iter(|| {
                    m.translate_tool_call(black_box(from), black_box(to), black_box(tc))
                        .unwrap()
                });
            },
        );
    }

    group.finish();
}

fn bench_message_mapping(c: &mut Criterion) {
    let mut group = c.benchmark_group("message_mapping");
    let matrix = ProjectionMatrix::new();

    let messages: Vec<Message> = vec![
        Message {
            role: MessageRole::System,
            content: "You are a helpful coding assistant.".into(),
        },
        Message {
            role: MessageRole::User,
            content: "Please fix the login bug in auth.rs".into(),
        },
        Message {
            role: MessageRole::Assistant,
            content: "I'll look at the auth module now.".into(),
        },
    ];

    let targets = [
        ("to_claude", Dialect::Abp, Dialect::Claude),
        ("to_openai", Dialect::Abp, Dialect::OpenAi),
        ("to_gemini", Dialect::Abp, Dialect::Gemini),
    ];

    for (name, from, to) in &targets {
        group.bench_with_input(
            BenchmarkId::new("dialect", name),
            &(&matrix, &messages),
            |b, (m, msgs)| {
                b.iter(|| {
                    m.map_messages(black_box(*from), black_box(*to), black_box(msgs))
                        .unwrap()
                });
            },
        );
    }

    group.finish();
}

fn bench_tool_definition_mapping(c: &mut Criterion) {
    let mut group = c.benchmark_group("tool_def_mapping");
    let matrix = ProjectionMatrix::new();

    let tools: Vec<ToolDefinitionIr> = (0..10)
        .map(|i| ToolDefinitionIr {
            name: format!("tool_{i}"),
            description: format!("A test tool number {i}"),
            parameters: serde_json::json!({"type": "object", "properties": {}}),
        })
        .collect();

    for count in [1, 5, 10] {
        let subset = &tools[..count];
        group.bench_with_input(
            BenchmarkId::new("abp_to_openai", count),
            &(&matrix, subset),
            |b, (m, t)| {
                b.iter(|| {
                    m.map_tool_definitions(
                        black_box(Dialect::Abp),
                        black_box(Dialect::OpenAi),
                        black_box(t),
                    )
                    .unwrap()
                });
            },
        );
    }

    group.finish();
}

fn bench_fidelity_check(c: &mut Criterion) {
    let matrix = ProjectionMatrix::new();

    c.bench_function("can_translate_all_pairs", |b| {
        b.iter(|| {
            for &from in Dialect::ALL {
                for &to in Dialect::ALL {
                    black_box(matrix.can_translate(from, to));
                }
            }
        });
    });
}

criterion_group!(
    benches,
    bench_matrix_construction,
    bench_work_order_translate,
    bench_tool_call_translate,
    bench_message_mapping,
    bench_tool_definition_mapping,
    bench_fidelity_check,
);
criterion_main!(benches);
