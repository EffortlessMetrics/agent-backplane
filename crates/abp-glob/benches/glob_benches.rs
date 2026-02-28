// SPDX-License-Identifier: MIT OR Apache-2.0
use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};

use abp_glob::IncludeExcludeGlobs;

fn patterns(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|x| x.to_string()).collect()
}

// ---------------------------------------------------------------------------
// Glob compilation: simple vs complex
// ---------------------------------------------------------------------------

fn bench_glob_compilation(c: &mut Criterion) {
    let simple_inc = patterns(&["src/**"]);
    let simple_exc = patterns(&["*.log"]);

    let complex_inc = patterns(&[
        "src/**",
        "tests/**",
        "benches/**",
        "examples/**",
        "docs/**",
        "crates/**",
        "lib/**",
        "bin/**",
        "scripts/**",
        "config/**",
    ]);
    let complex_exc = patterns(&[
        "target/**",
        "*.log",
        "*.tmp",
        "**/.git/**",
        "**/node_modules/**",
        "**/.env",
        "**/.env.*",
        "**/secret*",
        "**/dist/**",
        "**/__pycache__/**",
    ]);

    let mut group = c.benchmark_group("glob_compilation");
    group.bench_function("simple", |b| {
        b.iter(|| {
            IncludeExcludeGlobs::new(black_box(&simple_inc), black_box(&simple_exc)).unwrap()
        });
    });
    group.bench_function("complex_20_patterns", |b| {
        b.iter(|| {
            IncludeExcludeGlobs::new(black_box(&complex_inc), black_box(&complex_exc)).unwrap()
        });
    });
    group.finish();
}

// ---------------------------------------------------------------------------
// Path matching: short vs deep paths
// ---------------------------------------------------------------------------

fn bench_path_matching(c: &mut Criterion) {
    let globs = IncludeExcludeGlobs::new(
        &patterns(&["src/**", "tests/**"]),
        &patterns(&["**/.git/**", "*.log"]),
    )
    .unwrap();

    let mut group = c.benchmark_group("path_matching");
    group.bench_function("short_allowed", |b| {
        b.iter(|| globs.decide_str(black_box("src/lib.rs")));
    });
    group.bench_function("short_denied", |b| {
        b.iter(|| globs.decide_str(black_box("build.log")));
    });
    group.bench_function("deep_allowed", |b| {
        b.iter(|| globs.decide_str(black_box("src/a/b/c/d/e/f/g/module.rs")));
    });
    group.bench_function("deep_denied", |b| {
        b.iter(|| globs.decide_str(black_box("src/.git/objects/pack/abc123")));
    });
    group.finish();
}

// ---------------------------------------------------------------------------
// Include/exclude decision scaling: 1, 10, 100 rules
// ---------------------------------------------------------------------------

fn make_n_rules(n: usize) -> (Vec<String>, Vec<String>) {
    let include: Vec<String> = (0..n).map(|i| format!("dir_{i}/**")).collect();
    let exclude: Vec<String> = (0..n).map(|i| format!("dir_{i}/excluded_*")).collect();
    (include, exclude)
}

fn bench_decision_scaling(c: &mut Criterion) {
    let (inc_1, exc_1) = make_n_rules(1);
    let (inc_10, exc_10) = make_n_rules(10);
    let (inc_100, exc_100) = make_n_rules(100);

    let globs_1 = IncludeExcludeGlobs::new(&inc_1, &exc_1).unwrap();
    let globs_10 = IncludeExcludeGlobs::new(&inc_10, &exc_10).unwrap();
    let globs_100 = IncludeExcludeGlobs::new(&inc_100, &exc_100).unwrap();

    let path_hit = "dir_0/file.rs";
    let path_miss = "other/file.rs";

    let mut group = c.benchmark_group("decision_scaling");
    for (label, globs) in [("1_rule", &globs_1), ("10_rules", &globs_10), ("100_rules", &globs_100)] {
        group.bench_with_input(
            BenchmarkId::new(label, "hit"),
            &path_hit,
            |b, p| b.iter(|| globs.decide_str(black_box(p))),
        );
        group.bench_with_input(
            BenchmarkId::new(label, "miss"),
            &path_miss,
            |b, p| b.iter(|| globs.decide_str(black_box(p))),
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_glob_compilation,
    bench_path_matching,
    bench_decision_scaling,
);
criterion_main!(benches);
