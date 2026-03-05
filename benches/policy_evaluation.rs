// SPDX-License-Identifier: MIT OR Apache-2.0
//! Benchmark PolicyEngine compilation and evaluation at various policy sizes.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::path::Path;

use abp_core::PolicyProfile;
use abp_policy::PolicyEngine;

// ── Policy factories ────────────────────────────────────────────────────

fn policy_with_counts(tools: usize, paths: usize) -> PolicyProfile {
    PolicyProfile {
        allowed_tools: (0..tools).map(|i| format!("tool_{i}")).collect(),
        disallowed_tools: (0..tools / 2).map(|i| format!("banned_{i}")).collect(),
        deny_read: (0..paths).map(|i| format!("secret{i}/**")).collect(),
        deny_write: (0..paths).map(|i| format!("readonly{i}/**")).collect(),
        ..PolicyProfile::default()
    }
}

fn empty_policy() -> PolicyProfile {
    PolicyProfile::default()
}

fn realistic_policy() -> PolicyProfile {
    PolicyProfile {
        allowed_tools: vec![
            "Read".into(),
            "Write".into(),
            "Edit".into(),
            "Bash".into(),
            "Glob".into(),
            "Grep".into(),
            "WebFetch".into(),
        ],
        disallowed_tools: vec!["rm".into(), "sudo".into(), "curl".into()],
        deny_read: vec![
            "**/.env".into(),
            "**/secrets/**".into(),
            "**/*.pem".into(),
            "**/*.key".into(),
        ],
        deny_write: vec![
            "**/.git/**".into(),
            "**/node_modules/**".into(),
            "**/target/**".into(),
            "**/.env".into(),
        ],
        ..PolicyProfile::default()
    }
}

// ── Compilation scaling ─────────────────────────────────────────────────

fn bench_compile_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("policy_eval/compile_scaling");

    let sizes: Vec<(&str, PolicyProfile)> = vec![
        ("empty", empty_policy()),
        ("small_5t_5p", policy_with_counts(5, 5)),
        ("medium_50t_50p", policy_with_counts(50, 50)),
        ("large_200t_200p", policy_with_counts(200, 200)),
        ("realistic", realistic_policy()),
    ];

    for (name, policy) in &sizes {
        let total_rules = policy.allowed_tools.len()
            + policy.disallowed_tools.len()
            + policy.deny_read.len()
            + policy.deny_write.len();
        group.throughput(Throughput::Elements(total_rules as u64));
        group.bench_with_input(BenchmarkId::from_parameter(name), policy, |b, p| {
            b.iter(|| PolicyEngine::new(black_box(p)).unwrap());
        });
    }

    group.finish();
}

// ── Tool evaluation throughput ──────────────────────────────────────────

fn bench_tool_eval_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("policy_eval/tool_throughput");

    let tool_queries: Vec<String> = (0..200).map(|i| format!("tool_{i}")).collect();

    for (label, policy) in [
        ("small", policy_with_counts(10, 10)),
        ("medium", policy_with_counts(50, 50)),
        ("large", policy_with_counts(200, 200)),
    ] {
        let engine = PolicyEngine::new(&policy).unwrap();
        group.throughput(Throughput::Elements(tool_queries.len() as u64));

        group.bench_with_input(BenchmarkId::new("200_queries", label), &engine, |b, eng| {
            b.iter(|| {
                for t in &tool_queries {
                    black_box(eng.can_use_tool(t));
                }
            });
        });
    }

    group.finish();
}

// ── Path evaluation throughput ──────────────────────────────────────────

fn bench_path_eval_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("policy_eval/path_throughput");

    let read_paths: Vec<String> = (0..100)
        .map(|i| format!("src/module_{}/file_{}.rs", i / 10, i))
        .collect();
    let write_paths: Vec<String> = (0..100)
        .map(|i| format!("secret{}/deep/nested/data_{}.json", i % 50, i))
        .collect();

    for (label, policy) in [
        ("small", policy_with_counts(5, 5)),
        ("medium", policy_with_counts(50, 50)),
        ("large", policy_with_counts(200, 200)),
    ] {
        let engine = PolicyEngine::new(&policy).unwrap();
        group.throughput(Throughput::Elements(
            (read_paths.len() + write_paths.len()) as u64,
        ));

        group.bench_with_input(
            BenchmarkId::new("read_100_write_100", label),
            &engine,
            |b, eng| {
                b.iter(|| {
                    for p in &read_paths {
                        black_box(eng.can_read_path(Path::new(p)));
                    }
                    for p in &write_paths {
                        black_box(eng.can_write_path(Path::new(p)));
                    }
                });
            },
        );
    }

    group.finish();
}

// ── Worst-case: all denied paths ────────────────────────────────────────

fn bench_deny_heavy(c: &mut Criterion) {
    let mut group = c.benchmark_group("policy_eval/deny_heavy");

    let deny_read: Vec<String> = (0..500).map(|i| format!("locked{i}/**")).collect();
    let deny_write: Vec<String> = (0..500).map(|i| format!("frozen{i}/**")).collect();
    let policy = PolicyProfile {
        deny_read,
        deny_write,
        ..PolicyProfile::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();

    let paths: Vec<String> = (0..100)
        .map(|i| format!("locked{}/deep/file_{}.txt", i % 500, i))
        .collect();

    group.throughput(Throughput::Elements(paths.len() as u64));

    group.bench_function("read_100_paths_500_deny_rules", |b| {
        b.iter(|| {
            for p in &paths {
                black_box(engine.can_read_path(Path::new(p)));
            }
        });
    });

    group.bench_function("write_100_paths_500_deny_rules", |b| {
        b.iter(|| {
            for p in &paths {
                black_box(engine.can_write_path(Path::new(p)));
            }
        });
    });

    group.finish();
}

// ── Realistic mixed workload ────────────────────────────────────────────

fn bench_realistic_workload(c: &mut Criterion) {
    let mut group = c.benchmark_group("policy_eval/realistic_workload");

    let engine = PolicyEngine::new(&realistic_policy()).unwrap();

    let tools = ["Read", "Write", "Bash", "rm", "sudo", "unknown_tool"];
    let read_paths = [
        "src/main.rs",
        ".env",
        "secrets/api_key.pem",
        "README.md",
        "tests/integration.rs",
    ];
    let write_paths = [
        "src/lib.rs",
        ".git/config",
        "node_modules/pkg/index.js",
        "output.txt",
        "target/debug/binary",
    ];

    let total = tools.len() + read_paths.len() + write_paths.len();
    group.throughput(Throughput::Elements(total as u64));

    group.bench_function("mixed_checks", |b| {
        b.iter(|| {
            for t in &tools {
                black_box(engine.can_use_tool(t));
            }
            for p in &read_paths {
                black_box(engine.can_read_path(Path::new(p)));
            }
            for p in &write_paths {
                black_box(engine.can_write_path(Path::new(p)));
            }
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_compile_scaling,
    bench_tool_eval_throughput,
    bench_path_eval_throughput,
    bench_deny_heavy,
    bench_realistic_workload,
);
criterion_main!(benches);
