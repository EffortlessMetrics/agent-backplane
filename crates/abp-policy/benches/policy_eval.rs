// SPDX-License-Identifier: MIT OR Apache-2.0
use criterion::{Criterion, black_box, criterion_group, criterion_main};

use abp_core::PolicyProfile;
use abp_policy::PolicyEngine;
use std::path::Path;

fn sample_policy() -> PolicyProfile {
    PolicyProfile {
        allowed_tools: vec![
            "Read".into(),
            "Write".into(),
            "Bash".into(),
            "Grep".into(),
            "Glob".into(),
        ],
        disallowed_tools: vec!["DeleteFile".into(), "Bash*".into()],
        deny_read: vec![
            "**/.env".into(),
            "**/.env.*".into(),
            "**/id_rsa".into(),
            "**/secret*".into(),
        ],
        deny_write: vec![
            "**/locked/**".into(),
            "**/.git/**".into(),
            "**/node_modules/**".into(),
        ],
        allow_network: vec!["*.example.com".into()],
        deny_network: vec!["evil.example.com".into()],
        require_approval_for: vec!["Bash".into()],
    }
}

fn bench_policy_engine_new(c: &mut Criterion) {
    let policy = sample_policy();
    c.bench_function("policy_engine/new", |b| {
        b.iter(|| PolicyEngine::new(black_box(&policy)).unwrap())
    });
}

fn bench_can_use_tool(c: &mut Criterion) {
    let engine = PolicyEngine::new(&sample_policy()).unwrap();

    let mut group = c.benchmark_group("can_use_tool");
    group.bench_function("allowed", |b| {
        b.iter(|| engine.can_use_tool(black_box("Read")))
    });
    group.bench_function("denied_by_denylist", |b| {
        b.iter(|| engine.can_use_tool(black_box("BashExec")))
    });
    group.bench_function("denied_missing_allowlist", |b| {
        b.iter(|| engine.can_use_tool(black_box("WebSearch")))
    });
    group.finish();
}

fn bench_can_read_path(c: &mut Criterion) {
    let engine = PolicyEngine::new(&sample_policy()).unwrap();

    let mut group = c.benchmark_group("can_read_path");
    group.bench_function("allowed", |b| {
        b.iter(|| engine.can_read_path(black_box(Path::new("src/lib.rs"))))
    });
    group.bench_function("denied", |b| {
        b.iter(|| engine.can_read_path(black_box(Path::new(".env"))))
    });
    group.bench_function("deep_path_allowed", |b| {
        b.iter(|| engine.can_read_path(black_box(Path::new("src/a/b/c/d/e/module.rs"))))
    });
    group.bench_function("deep_path_denied", |b| {
        b.iter(|| engine.can_read_path(black_box(Path::new("home/.ssh/id_rsa"))))
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_policy_engine_new,
    bench_can_use_tool,
    bench_can_read_path,
);
criterion_main!(benches);
