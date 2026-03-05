// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive benchmarks for policy evaluation with varying rule counts
//! (10, 100, 1000 rules), covering compilation, tool checks, path checks,
//! and batch evaluation scenarios.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use std::path::Path;

use abp_core::PolicyProfile;
use abp_policy::PolicyEngine;

// ── Helpers ─────────────────────────────────────────────────────────────

fn make_policy(n: usize) -> PolicyProfile {
    let disallowed: Vec<String> = (0..n).map(|i| format!("DeniedTool{i}*")).collect();
    let deny_write: Vec<String> = (0..n).map(|i| format!("secret{i}/**")).collect();
    let deny_read: Vec<String> = (0..n).map(|i| format!("private{i}/**")).collect();
    let allow_network: Vec<String> = (0..n.min(20))
        .map(|i| format!("api{i}.example.com"))
        .collect();
    let deny_network: Vec<String> = (0..n.min(20))
        .map(|i| format!("evil{i}.example.com"))
        .collect();
    PolicyProfile {
        allowed_tools: vec!["*".into()],
        disallowed_tools: disallowed,
        deny_read,
        deny_write,
        allow_network,
        deny_network,
        ..PolicyProfile::default()
    }
}

fn make_mixed_policy(n: usize) -> PolicyProfile {
    // Realistic policy: some specific allows, many denies, approval gates
    let allowed: Vec<String> = (0..n / 2).map(|i| format!("AllowedTool{i}")).collect();
    let disallowed: Vec<String> = (0..n).map(|i| format!("BadTool{i}*")).collect();
    let deny_write: Vec<String> = (0..n).map(|i| format!("protected{i}/**")).collect();
    let deny_read: Vec<String> = (0..n / 2).map(|i| format!("classified{i}/**")).collect();
    let require_approval: Vec<String> = (0..n.min(10))
        .map(|i| format!("SensitiveTool{i}"))
        .collect();
    PolicyProfile {
        allowed_tools: allowed,
        disallowed_tools: disallowed,
        deny_read,
        deny_write,
        require_approval_for: require_approval,
        ..PolicyProfile::default()
    }
}

// ── Policy compilation ──────────────────────────────────────────────────

fn bench_policy_compilation(c: &mut Criterion) {
    let mut group = c.benchmark_group("policy_compilation");

    for rule_count in [10, 100, 1000] {
        let policy = make_policy(rule_count);
        group.bench_with_input(BenchmarkId::new("uniform", rule_count), &policy, |b, p| {
            b.iter(|| PolicyEngine::new(black_box(p)).unwrap());
        });
    }

    for rule_count in [10, 100, 1000] {
        let policy = make_mixed_policy(rule_count);
        group.bench_with_input(BenchmarkId::new("mixed", rule_count), &policy, |b, p| {
            b.iter(|| PolicyEngine::new(black_box(p)).unwrap());
        });
    }

    group.finish();
}

// ── Tool checks ─────────────────────────────────────────────────────────

fn bench_tool_check(c: &mut Criterion) {
    let mut group = c.benchmark_group("policy_tool_check");

    for rule_count in [10, 100, 1000] {
        let policy = make_policy(rule_count);
        let engine = PolicyEngine::new(&policy).unwrap();

        // Allowed tool (not in deny list)
        group.bench_with_input(
            BenchmarkId::new("allowed", rule_count),
            &engine,
            |b, eng| {
                b.iter(|| eng.can_use_tool(black_box("ReadFile")));
            },
        );

        // Denied tool (matches first deny pattern)
        group.bench_with_input(
            BenchmarkId::new("denied_first", rule_count),
            &engine,
            |b, eng| {
                b.iter(|| eng.can_use_tool(black_box("DeniedTool0Match")));
            },
        );

        // Denied tool (matches last deny pattern — worst-case scan)
        let last_tool = format!("DeniedTool{}Match", rule_count - 1);
        group.bench_with_input(
            BenchmarkId::new("denied_last", rule_count),
            &(&engine, last_tool.clone()),
            |b, (eng, tool)| {
                b.iter(|| eng.can_use_tool(black_box(tool)));
            },
        );
    }

    group.finish();
}

// ── Path checks ─────────────────────────────────────────────────────────

fn bench_path_check(c: &mut Criterion) {
    let mut group = c.benchmark_group("policy_path_check");

    for rule_count in [10, 100, 1000] {
        let policy = make_policy(rule_count);
        let engine = PolicyEngine::new(&policy).unwrap();

        // Write to an allowed path
        group.bench_with_input(
            BenchmarkId::new("write_allowed", rule_count),
            &engine,
            |b, eng| {
                b.iter(|| eng.can_write_path(black_box(Path::new("src/main.rs"))));
            },
        );

        // Write to a denied path (matches first pattern)
        group.bench_with_input(
            BenchmarkId::new("write_denied_first", rule_count),
            &engine,
            |b, eng| {
                b.iter(|| eng.can_write_path(black_box(Path::new("secret0/data.txt"))));
            },
        );

        // Write to a denied path (matches last pattern — worst-case)
        let denied_path = format!("secret{}/deep/nested/data.txt", rule_count - 1);
        group.bench_with_input(
            BenchmarkId::new("write_denied_last", rule_count),
            &(&engine, denied_path.clone()),
            |b, (eng, path)| {
                b.iter(|| eng.can_write_path(black_box(Path::new(path))));
            },
        );

        // Read denied path
        group.bench_with_input(
            BenchmarkId::new("read_denied", rule_count),
            &engine,
            |b, eng| {
                b.iter(|| eng.can_read_path(black_box(Path::new("private0/keys.pem"))));
            },
        );

        // Read allowed path (deep nesting)
        group.bench_with_input(
            BenchmarkId::new("read_allowed_deep", rule_count),
            &engine,
            |b, eng| {
                b.iter(|| {
                    eng.can_read_path(black_box(Path::new("src/deeply/nested/path/to/module.rs")))
                });
            },
        );
    }

    group.finish();
}

// ── Batch evaluation (simulating a sequence of tool calls) ──────────────

fn bench_batch_evaluation(c: &mut Criterion) {
    let mut group = c.benchmark_group("policy_batch_eval");

    for rule_count in [10, 100, 1000] {
        let policy = make_policy(rule_count);
        let engine = PolicyEngine::new(&policy).unwrap();

        // Simulate checking 100 tool calls in sequence
        let tools: Vec<String> = (0..100)
            .map(|i| {
                if i % 5 == 0 {
                    format!("DeniedTool{i}Match")
                } else {
                    format!("SafeTool{i}")
                }
            })
            .collect();

        group.throughput(Throughput::Elements(100));
        group.bench_with_input(
            BenchmarkId::new("100_tool_calls", rule_count),
            &(&engine, &tools),
            |b, (eng, ts)| {
                b.iter(|| {
                    let mut allowed = 0u32;
                    for t in *ts {
                        if eng.can_use_tool(black_box(t)).allowed {
                            allowed += 1;
                        }
                    }
                    black_box(allowed);
                });
            },
        );

        // Simulate checking 100 path writes
        let paths: Vec<String> = (0..100)
            .map(|i| {
                if i % 5 == 0 {
                    format!("secret{}/file.txt", i % rule_count)
                } else {
                    format!("src/module{i}/lib.rs")
                }
            })
            .collect();

        group.throughput(Throughput::Elements(100));
        group.bench_with_input(
            BenchmarkId::new("100_path_writes", rule_count),
            &(&engine, &paths),
            |b, (eng, ps)| {
                b.iter(|| {
                    let mut allowed = 0u32;
                    for p in *ps {
                        if eng.can_write_path(black_box(Path::new(p))).allowed {
                            allowed += 1;
                        }
                    }
                    black_box(allowed);
                });
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
    bench_batch_evaluation,
);
criterion_main!(benches);
