// SPDX-License-Identifier: MIT OR Apache-2.0
//! Benchmarks for policy evaluation with varying rule complexity.

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use std::path::Path;

use abp_core::PolicyProfile;
use abp_policy::PolicyEngine;

/// Build a policy with `n` disallowed tool patterns and `n` deny-write globs.
fn make_policy(n: usize) -> PolicyProfile {
    let disallowed: Vec<String> = (0..n).map(|i| format!("DeniedTool{i}*")).collect();
    let deny_write: Vec<String> = (0..n).map(|i| format!("secret{i}/**")).collect();
    let deny_read: Vec<String> = (0..n).map(|i| format!("private{i}/**")).collect();
    PolicyProfile {
        allowed_tools: vec!["*".into()],
        disallowed_tools: disallowed,
        deny_read,
        deny_write,
        ..PolicyProfile::default()
    }
}

fn bench_policy_compilation(c: &mut Criterion) {
    let mut group = c.benchmark_group("policy_compilation");

    for rule_count in [1, 10, 100] {
        let policy = make_policy(rule_count);
        group.bench_with_input(BenchmarkId::new("rules", rule_count), &policy, |b, p| {
            b.iter(|| PolicyEngine::new(black_box(p)).unwrap());
        });
    }

    group.finish();
}

fn bench_tool_check(c: &mut Criterion) {
    let mut group = c.benchmark_group("policy_tool_check");

    for rule_count in [1, 10, 100] {
        let policy = make_policy(rule_count);
        let engine = PolicyEngine::new(&policy).unwrap();

        // Check a tool that IS allowed (not in deny list).
        group.bench_with_input(
            BenchmarkId::new("allowed", rule_count),
            &engine,
            |b, eng| {
                b.iter(|| eng.can_use_tool(black_box("Read")));
            },
        );

        // Check a tool that IS denied.
        group.bench_with_input(BenchmarkId::new("denied", rule_count), &engine, |b, eng| {
            b.iter(|| eng.can_use_tool(black_box("DeniedTool0Match")));
        });
    }

    group.finish();
}

fn bench_path_check(c: &mut Criterion) {
    let mut group = c.benchmark_group("policy_path_check");

    for rule_count in [1, 10, 100] {
        let policy = make_policy(rule_count);
        let engine = PolicyEngine::new(&policy).unwrap();

        // Check a path that IS allowed.
        group.bench_with_input(
            BenchmarkId::new("write_allowed", rule_count),
            &engine,
            |b, eng| {
                b.iter(|| eng.can_write_path(black_box(Path::new("src/main.rs"))));
            },
        );

        // Check a path that IS denied.
        group.bench_with_input(
            BenchmarkId::new("write_denied", rule_count),
            &engine,
            |b, eng| {
                b.iter(|| eng.can_write_path(black_box(Path::new("secret0/data.txt"))));
            },
        );

        // Check read path.
        group.bench_with_input(
            BenchmarkId::new("read_denied", rule_count),
            &engine,
            |b, eng| {
                b.iter(|| eng.can_read_path(black_box(Path::new("private0/keys.pem"))));
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_policy_compilation,
    bench_tool_check,
    bench_path_check,
);
criterion_main!(benches);
