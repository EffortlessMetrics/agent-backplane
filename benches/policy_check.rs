// SPDX-License-Identifier: MIT OR Apache-2.0
//! Benchmark policy engine decisions at scale.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use std::path::Path;

use abp_core::PolicyProfile;
use abp_policy::PolicyEngine;

fn make_policy(n_tools: usize, n_paths: usize) -> PolicyProfile {
    PolicyProfile {
        allowed_tools: vec!["*".into()],
        disallowed_tools: (0..n_tools).map(|i| format!("Denied{i}*")).collect(),
        deny_read: (0..n_paths).map(|i| format!("secret{i}/**")).collect(),
        deny_write: (0..n_paths).map(|i| format!("locked{i}/**")).collect(),
        ..PolicyProfile::default()
    }
}

fn bench_policy_compilation_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("policy_compile_scaling");

    for rule_count in [1, 10, 50, 100, 500] {
        let policy = make_policy(rule_count, rule_count);
        group.bench_with_input(BenchmarkId::new("rules", rule_count), &policy, |b, p| {
            b.iter(|| PolicyEngine::new(black_box(p)).unwrap());
        });
    }

    group.finish();
}

fn bench_tool_check_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("policy_tool_check_scaling");

    for rule_count in [1, 10, 100, 500] {
        let policy = make_policy(rule_count, 0);
        let engine = PolicyEngine::new(&policy).unwrap();

        // Best case: tool is allowed (no deny match)
        group.bench_with_input(
            BenchmarkId::new("allowed", rule_count),
            &engine,
            |b, eng| {
                b.iter(|| eng.can_use_tool(black_box("AllowedTool")));
            },
        );

        // First-rule match: denied by first pattern
        group.bench_with_input(
            BenchmarkId::new("denied_first", rule_count),
            &engine,
            |b, eng| {
                b.iter(|| eng.can_use_tool(black_box("Denied0Match")));
            },
        );

        // Last-rule match: denied by last pattern (worst case scan)
        let last_tool = format!("Denied{}Match", rule_count - 1);
        group.bench_with_input(
            BenchmarkId::new("denied_last", rule_count),
            &(engine.clone(), last_tool.clone()),
            |b, (eng, tool)| {
                b.iter(|| eng.can_use_tool(black_box(tool)));
            },
        );
    }

    group.finish();
}

fn bench_path_write_check_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("policy_write_check_scaling");

    for rule_count in [1, 10, 100, 500] {
        let policy = make_policy(0, rule_count);
        let engine = PolicyEngine::new(&policy).unwrap();

        group.bench_with_input(
            BenchmarkId::new("write_allowed", rule_count),
            &engine,
            |b, eng| {
                b.iter(|| eng.can_write_path(black_box(Path::new("src/main.rs"))));
            },
        );

        group.bench_with_input(
            BenchmarkId::new("write_denied_first", rule_count),
            &engine,
            |b, eng| {
                b.iter(|| eng.can_write_path(black_box(Path::new("locked0/data.txt"))));
            },
        );

        group.bench_with_input(
            BenchmarkId::new("read_denied_first", rule_count),
            &engine,
            |b, eng| {
                b.iter(|| eng.can_read_path(black_box(Path::new("secret0/keys.pem"))));
            },
        );
    }

    group.finish();
}

fn bench_batch_tool_evaluation(c: &mut Criterion) {
    let mut group = c.benchmark_group("policy_batch_tools");

    let tools: Vec<String> = (0..100).map(|i| format!("Tool{i}")).collect();

    for rule_count in [10, 100, 500] {
        let policy = make_policy(rule_count, 0);
        let engine = PolicyEngine::new(&policy).unwrap();

        group.throughput(Throughput::Elements(tools.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("check_100_tools", rule_count),
            &(engine, &tools),
            |b, (eng, ts)| {
                b.iter(|| {
                    for t in *ts {
                        black_box(eng.can_use_tool(t));
                    }
                });
            },
        );
    }

    group.finish();
}

fn bench_batch_path_evaluation(c: &mut Criterion) {
    let mut group = c.benchmark_group("policy_batch_paths");

    let paths: Vec<String> = (0..100)
        .map(|i| format!("project/src/module{}/file{}.rs", i / 10, i))
        .collect();

    for rule_count in [10, 100, 500] {
        let policy = make_policy(0, rule_count);
        let engine = PolicyEngine::new(&policy).unwrap();

        group.throughput(Throughput::Elements(paths.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("write_100_paths", rule_count),
            &(engine.clone(), &paths),
            |b, (eng, ps)| {
                b.iter(|| {
                    for p in *ps {
                        black_box(eng.can_write_path(Path::new(p)));
                    }
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("read_100_paths", rule_count),
            &(engine, &paths),
            |b, (eng, ps)| {
                b.iter(|| {
                    for p in *ps {
                        black_box(eng.can_read_path(Path::new(p)));
                    }
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_policy_compilation_scaling,
    bench_tool_check_scaling,
    bench_path_write_check_scaling,
    bench_batch_tool_evaluation,
    bench_batch_path_evaluation,
);
criterion_main!(benches);
