// SPDX-License-Identifier: MIT OR Apache-2.0
//! Benchmark policy evaluation (allow/deny decisions per second) at varying scales.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::path::Path;

use abp_core::PolicyProfile;
use abp_policy::PolicyEngine;

// ── Policy builders ─────────────────────────────────────────────────────

fn small_policy() -> PolicyProfile {
    PolicyProfile {
        allowed_tools: vec!["read_file".into(), "write_file".into(), "search".into()],
        disallowed_tools: vec!["rm".into(), "sudo".into()],
        deny_read: vec!["secrets/**".into()],
        deny_write: vec![".env".into(), "*.pem".into()],
        ..PolicyProfile::default()
    }
}

fn medium_policy() -> PolicyProfile {
    let allowed: Vec<String> = (0..20).map(|i| format!("tool_{i}")).collect();
    let disallowed: Vec<String> = (0..30).map(|i| format!("DeniedTool{i}*")).collect();
    let deny_read: Vec<String> = (0..20).map(|i| format!("private{i}/**")).collect();
    let deny_write: Vec<String> = (0..20).map(|i| format!("secret{i}/**")).collect();
    PolicyProfile {
        allowed_tools: allowed,
        disallowed_tools: disallowed,
        deny_read,
        deny_write,
        ..PolicyProfile::default()
    }
}

fn large_policy() -> PolicyProfile {
    let allowed: Vec<String> = (0..100).map(|i| format!("tool_{i}")).collect();
    let disallowed: Vec<String> = (0..200).map(|i| format!("DeniedTool{i}*")).collect();
    let deny_read: Vec<String> = (0..100).map(|i| format!("private{i}/**")).collect();
    let deny_write: Vec<String> = (0..100).map(|i| format!("secret{i}/**")).collect();
    PolicyProfile {
        allowed_tools: allowed,
        disallowed_tools: disallowed,
        deny_read,
        deny_write,
        ..PolicyProfile::default()
    }
}

// ── Compilation benchmark ───────────────────────────────────────────────

fn bench_compile(c: &mut Criterion) {
    let mut group = c.benchmark_group("policy_compile");

    let cases: Vec<(&str, PolicyProfile)> = vec![
        ("small", small_policy()),
        ("medium", medium_policy()),
        ("large", large_policy()),
    ];

    for (name, policy) in &cases {
        group.bench_with_input(BenchmarkId::from_parameter(name), policy, |b, p| {
            b.iter(|| PolicyEngine::new(black_box(p)).unwrap());
        });
    }

    group.finish();
}

// ── Tool allow/deny decisions ───────────────────────────────────────────

fn bench_tool_decisions(c: &mut Criterion) {
    let mut group = c.benchmark_group("policy_tool_decision");

    let cases: Vec<(&str, PolicyEngine)> = vec![
        ("small", PolicyEngine::new(&small_policy()).unwrap()),
        ("medium", PolicyEngine::new(&medium_policy()).unwrap()),
        ("large", PolicyEngine::new(&large_policy()).unwrap()),
    ];

    for (name, engine) in &cases {
        // Allowed tool
        group.bench_with_input(BenchmarkId::new("allowed", name), engine, |b, eng| {
            b.iter(|| eng.can_use_tool(black_box("read_file")));
        });

        // Denied tool
        group.bench_with_input(BenchmarkId::new("denied", name), engine, |b, eng| {
            b.iter(|| eng.can_use_tool(black_box("DeniedTool0Match")));
        });

        // Unknown tool (not in any list)
        group.bench_with_input(BenchmarkId::new("unknown", name), engine, |b, eng| {
            b.iter(|| eng.can_use_tool(black_box("totally_unknown_tool")));
        });
    }

    group.finish();
}

// ── Path allow/deny decisions ───────────────────────────────────────────

fn bench_path_decisions(c: &mut Criterion) {
    let mut group = c.benchmark_group("policy_path_decision");

    let cases: Vec<(&str, PolicyEngine)> = vec![
        ("small", PolicyEngine::new(&small_policy()).unwrap()),
        ("medium", PolicyEngine::new(&medium_policy()).unwrap()),
        ("large", PolicyEngine::new(&large_policy()).unwrap()),
    ];

    for (name, engine) in &cases {
        // Allowed write path
        group.bench_with_input(BenchmarkId::new("write_allowed", name), engine, |b, eng| {
            b.iter(|| eng.can_write_path(black_box(Path::new("src/main.rs"))));
        });

        // Denied write path
        group.bench_with_input(BenchmarkId::new("write_denied", name), engine, |b, eng| {
            b.iter(|| eng.can_write_path(black_box(Path::new("secret0/data.txt"))));
        });

        // Allowed read path
        group.bench_with_input(BenchmarkId::new("read_allowed", name), engine, |b, eng| {
            b.iter(|| eng.can_read_path(black_box(Path::new("src/lib.rs"))));
        });

        // Denied read path
        group.bench_with_input(BenchmarkId::new("read_denied", name), engine, |b, eng| {
            b.iter(|| eng.can_read_path(black_box(Path::new("private0/keys.pem"))));
        });
    }

    group.finish();
}

// ── Batch evaluation throughput ─────────────────────────────────────────

fn bench_batch_eval(c: &mut Criterion) {
    let mut group = c.benchmark_group("policy_batch_eval");

    let paths: Vec<String> = (0..100)
        .map(|i| format!("src/module_{i}/file.rs"))
        .collect();
    let tools: Vec<String> = (0..50).map(|i| format!("tool_{i}")).collect();

    for (label, policy) in [
        ("small", small_policy()),
        ("medium", medium_policy()),
        ("large", large_policy()),
    ] {
        let engine = PolicyEngine::new(&policy).unwrap();

        group.throughput(Throughput::Elements((paths.len() + tools.len()) as u64));
        group.bench_with_input(BenchmarkId::from_parameter(label), &engine, |b, eng| {
            b.iter(|| {
                for p in &paths {
                    let _ = eng.can_write_path(black_box(Path::new(p)));
                    let _ = eng.can_read_path(black_box(Path::new(p)));
                }
                for t in &tools {
                    let _ = eng.can_use_tool(black_box(t));
                }
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_compile,
    bench_tool_decisions,
    bench_path_decisions,
    bench_batch_eval,
);
criterion_main!(benches);
