// SPDX-License-Identifier: MIT OR Apache-2.0
//! Benchmark WorkOrder serialize/deserialize.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};

use abp_core::{
    Capability, CapabilityRequirement, CapabilityRequirements, ContextPacket, ContextSnippet,
    ExecutionLane, MinSupport, PolicyProfile, WorkOrder, WorkOrderBuilder,
};

fn simple_work_order() -> WorkOrder {
    WorkOrderBuilder::new("Simple task")
        .root("/tmp/workspace")
        .build()
}

fn medium_work_order() -> WorkOrder {
    WorkOrderBuilder::new("Medium complexity task: refactor the authentication module")
        .root("/home/user/project")
        .lane(ExecutionLane::WorkspaceFirst)
        .model("gpt-4")
        .max_turns(20)
        .build()
}

fn complex_work_order() -> WorkOrder {
    let mut wo = WorkOrderBuilder::new(
        "Complex task: implement OAuth2 with PKCE flow, add comprehensive tests, update docs",
    )
    .root("/home/user/large-project")
    .lane(ExecutionLane::WorkspaceFirst)
    .model("claude-sonnet-4-20250514")
    .max_turns(50)
    .build();

    wo.policy = PolicyProfile {
        allowed_tools: vec![
            "Read".into(),
            "Write".into(),
            "Edit".into(),
            "Bash".into(),
            "Glob".into(),
            "Grep".into(),
        ],
        disallowed_tools: vec!["WebFetch".into(), "WebSearch".into()],
        deny_read: vec!["**/.env".into(), "**/secrets/**".into()],
        deny_write: vec!["**/.git/**".into(), "**/node_modules/**".into()],
        ..PolicyProfile::default()
    };

    wo.requirements = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolWrite,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolBash,
                min_support: MinSupport::Emulated,
            },
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Emulated,
            },
        ],
    };

    wo.context = ContextPacket {
        files: (0..20).map(|i| format!("src/module{i}/mod.rs")).collect(),
        snippets: vec![
            ContextSnippet {
                name: "architecture".into(),
                content: "The project uses a hexagonal architecture with ports and adapters."
                    .into(),
            },
            ContextSnippet {
                name: "conventions".into(),
                content: "Use Result<T, Error> for all fallible functions. Prefer thiserror."
                    .into(),
            },
        ],
    };

    wo
}

fn bench_work_order_serialize(c: &mut Criterion) {
    let mut group = c.benchmark_group("work_order_serialize");

    let cases: Vec<(&str, WorkOrder)> = vec![
        ("simple", simple_work_order()),
        ("medium", medium_work_order()),
        ("complex", complex_work_order()),
    ];

    for (name, wo) in &cases {
        let json = serde_json::to_string(wo).unwrap();
        group.throughput(Throughput::Bytes(json.len() as u64));

        group.bench_with_input(BenchmarkId::new("complexity", name), wo, |b, w| {
            b.iter(|| serde_json::to_string(black_box(w)).unwrap());
        });
    }

    group.finish();
}

fn bench_work_order_deserialize(c: &mut Criterion) {
    let mut group = c.benchmark_group("work_order_deserialize");

    let cases: Vec<(&str, String)> = vec![
        (
            "simple",
            serde_json::to_string(&simple_work_order()).unwrap(),
        ),
        (
            "medium",
            serde_json::to_string(&medium_work_order()).unwrap(),
        ),
        (
            "complex",
            serde_json::to_string(&complex_work_order()).unwrap(),
        ),
    ];

    for (name, json) in &cases {
        group.throughput(Throughput::Bytes(json.len() as u64));

        group.bench_with_input(BenchmarkId::new("complexity", name), json, |b, j| {
            b.iter(|| serde_json::from_str::<WorkOrder>(black_box(j)).unwrap());
        });
    }

    group.finish();
}

fn bench_work_order_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("work_order_roundtrip");

    let cases: Vec<(&str, WorkOrder)> = vec![
        ("simple", simple_work_order()),
        ("medium", medium_work_order()),
        ("complex", complex_work_order()),
    ];

    for (name, wo) in &cases {
        group.bench_with_input(BenchmarkId::new("complexity", name), wo, |b, w| {
            b.iter(|| {
                let json = serde_json::to_string(black_box(w)).unwrap();
                serde_json::from_str::<WorkOrder>(&json).unwrap()
            });
        });
    }

    group.finish();
}

fn bench_work_order_builder(c: &mut Criterion) {
    let mut group = c.benchmark_group("work_order_builder");

    group.bench_function("minimal", |b| {
        b.iter(|| {
            black_box(WorkOrderBuilder::new("task").build());
        });
    });

    group.bench_function("with_options", |b| {
        b.iter(|| {
            black_box(
                WorkOrderBuilder::new("Refactor auth")
                    .root("/tmp/ws")
                    .lane(ExecutionLane::WorkspaceFirst)
                    .model("gpt-4")
                    .max_turns(20)
                    .build(),
            );
        });
    });

    group.finish();
}

fn bench_work_order_pretty_vs_compact(c: &mut Criterion) {
    let mut group = c.benchmark_group("work_order_format");
    let wo = complex_work_order();

    group.bench_function("compact", |b| {
        b.iter(|| serde_json::to_string(black_box(&wo)).unwrap());
    });

    group.bench_function("pretty", |b| {
        b.iter(|| serde_json::to_string_pretty(black_box(&wo)).unwrap());
    });

    group.bench_function("to_value", |b| {
        b.iter(|| serde_json::to_value(black_box(&wo)).unwrap());
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_work_order_serialize,
    bench_work_order_deserialize,
    bench_work_order_roundtrip,
    bench_work_order_builder,
    bench_work_order_pretty_vs_compact,
);
criterion_main!(benches);
