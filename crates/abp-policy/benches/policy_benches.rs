// SPDX-License-Identifier: MIT OR Apache-2.0
use criterion::{Criterion, black_box, criterion_group, criterion_main};

use abp_core::PolicyProfile;
use abp_policy::PolicyEngine;
use std::path::Path;

// ---------------------------------------------------------------------------
// Policy fixtures
// ---------------------------------------------------------------------------

fn simple_policy() -> PolicyProfile {
    PolicyProfile {
        disallowed_tools: vec!["Bash".into()],
        deny_write: vec!["**/.git/**".into()],
        ..PolicyProfile::default()
    }
}

fn complex_policy() -> PolicyProfile {
    PolicyProfile {
        allowed_tools: (0..20).map(|i| format!("Tool_{i}")).collect(),
        disallowed_tools: vec![
            "Bash*".into(),
            "Delete*".into(),
            "Exec*".into(),
            "Rm*".into(),
            "Shutdown*".into(),
        ],
        deny_read: vec![
            "**/.env".into(),
            "**/.env.*".into(),
            "**/id_rsa".into(),
            "**/secret*".into(),
            "**/.aws/**".into(),
            "**/credentials*".into(),
            "**/.ssh/**".into(),
            "**/private_key*".into(),
        ],
        deny_write: vec![
            "**/locked/**".into(),
            "**/.git/**".into(),
            "**/node_modules/**".into(),
            "**/vendor/**".into(),
            "**/.cargo/**".into(),
            "**/dist/**".into(),
        ],
        allow_network: vec!["*.example.com".into()],
        deny_network: vec!["evil.example.com".into()],
        require_approval_for: vec!["Bash".into(), "DeleteFile".into()],
    }
}

// ---------------------------------------------------------------------------
// PolicyEngine compilation
// ---------------------------------------------------------------------------

fn bench_policy_compilation(c: &mut Criterion) {
    let simple = simple_policy();
    let complex = complex_policy();

    let mut group = c.benchmark_group("policy_compile");
    group.bench_function("simple", |b| {
        b.iter(|| PolicyEngine::new(black_box(&simple)).unwrap());
    });
    group.bench_function("complex", |b| {
        b.iter(|| PolicyEngine::new(black_box(&complex)).unwrap());
    });
    group.finish();
}

// ---------------------------------------------------------------------------
// Tool checks: simple vs complex policy
// ---------------------------------------------------------------------------

fn bench_tool_check(c: &mut Criterion) {
    let simple_engine = PolicyEngine::new(&simple_policy()).unwrap();
    let complex_engine = PolicyEngine::new(&complex_policy()).unwrap();

    let mut group = c.benchmark_group("tool_check");
    group.bench_function("simple_allowed", |b| {
        b.iter(|| simple_engine.can_use_tool(black_box("Read")));
    });
    group.bench_function("simple_denied", |b| {
        b.iter(|| simple_engine.can_use_tool(black_box("Bash")));
    });
    group.bench_function("complex_allowed", |b| {
        b.iter(|| complex_engine.can_use_tool(black_box("Tool_5")));
    });
    group.bench_function("complex_denied_denylist", |b| {
        b.iter(|| complex_engine.can_use_tool(black_box("BashExec")));
    });
    group.bench_function("complex_denied_missing", |b| {
        b.iter(|| complex_engine.can_use_tool(black_box("UnknownTool")));
    });
    group.finish();
}

// ---------------------------------------------------------------------------
// Read / write path checks
// ---------------------------------------------------------------------------

fn bench_path_checks(c: &mut Criterion) {
    let engine = PolicyEngine::new(&complex_policy()).unwrap();

    let mut group = c.benchmark_group("read_path");
    group.bench_function("allowed", |b| {
        b.iter(|| engine.can_read_path(black_box(Path::new("src/lib.rs"))));
    });
    group.bench_function("denied_env", |b| {
        b.iter(|| engine.can_read_path(black_box(Path::new(".env"))));
    });
    group.bench_function("denied_deep", |b| {
        b.iter(|| engine.can_read_path(black_box(Path::new("home/.ssh/id_rsa"))));
    });
    group.finish();

    let mut group = c.benchmark_group("write_path");
    group.bench_function("allowed", |b| {
        b.iter(|| engine.can_write_path(black_box(Path::new("src/main.rs"))));
    });
    group.bench_function("denied_git", |b| {
        b.iter(|| engine.can_write_path(black_box(Path::new(".git/config"))));
    });
    group.bench_function("denied_deep", |b| {
        b.iter(|| engine.can_write_path(black_box(Path::new("vendor/pkg/a/b/c.go"))));
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_policy_compilation,
    bench_tool_check,
    bench_path_checks,
);
criterion_main!(benches);
