// SPDX-License-Identifier: MIT OR Apache-2.0
//! Benchmark glob pattern matching.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

use abp_glob::{build_globset, IncludeExcludeGlobs, MatchDecision};

fn patterns(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|x| x.to_string()).collect()
}

fn bench_glob_compilation(c: &mut Criterion) {
    let mut group = c.benchmark_group("glob_compilation");

    let sizes: Vec<(&str, Vec<String>)> = vec![
        ("1_pattern", patterns(&["src/**"])),
        (
            "5_patterns",
            patterns(&["src/**", "tests/**", "benches/**", "docs/**", "*.toml"]),
        ),
        (
            "20_patterns",
            (0..20).map(|i| format!("dir{i}/**/*.rs")).collect(),
        ),
        (
            "100_patterns",
            (0..100).map(|i| format!("path{i}/**/*.txt")).collect(),
        ),
    ];

    for (name, pats) in &sizes {
        group.bench_with_input(BenchmarkId::new("include", name), pats, |b, p| {
            b.iter(|| IncludeExcludeGlobs::new(black_box(p), &[]).unwrap());
        });
    }

    group.finish();
}

fn bench_glob_decide_str(c: &mut Criterion) {
    let mut group = c.benchmark_group("glob_decide_str");

    let test_paths: Vec<String> = (0..100)
        .map(|i| format!("src/module{}/file{}.rs", i / 10, i))
        .collect();

    // Simple: include only
    let simple = IncludeExcludeGlobs::new(&patterns(&["src/**"]), &[]).unwrap();
    group.bench_function("include_only/allowed", |b| {
        b.iter(|| {
            for p in &test_paths {
                black_box(simple.decide_str(p));
            }
        });
    });

    // Complex: include + exclude
    let complex = IncludeExcludeGlobs::new(
        &patterns(&["src/**", "tests/**"]),
        &patterns(&["src/generated/**", "tests/fixtures/**"]),
    )
    .unwrap();

    group.bench_function("include_exclude/mixed", |b| {
        b.iter(|| {
            for p in &test_paths {
                black_box(complex.decide_str(p));
            }
        });
    });

    // Many patterns
    let many_include: Vec<String> = (0..50).map(|i| format!("dir{i}/**")).collect();
    let many_exclude: Vec<String> = (0..50).map(|i| format!("dir{i}/tmp/**")).collect();
    let many = IncludeExcludeGlobs::new(&many_include, &many_exclude).unwrap();

    group.bench_function("50_include_50_exclude", |b| {
        b.iter(|| {
            for p in &test_paths {
                black_box(many.decide_str(p));
            }
        });
    });

    group.finish();
}

fn bench_glob_match_decision_types(c: &mut Criterion) {
    let mut group = c.benchmark_group("glob_decision_types");

    let globs = IncludeExcludeGlobs::new(
        &patterns(&["src/**", "tests/**"]),
        &patterns(&["src/secret/**"]),
    )
    .unwrap();

    group.bench_function("allowed", |b| {
        b.iter(|| {
            assert_eq!(
                globs.decide_str(black_box("src/lib.rs")),
                MatchDecision::Allowed
            );
        });
    });

    group.bench_function("denied_by_exclude", |b| {
        b.iter(|| {
            assert_eq!(
                globs.decide_str(black_box("src/secret/key.pem")),
                MatchDecision::DeniedByExclude
            );
        });
    });

    group.bench_function("denied_by_missing_include", |b| {
        b.iter(|| {
            assert_eq!(
                globs.decide_str(black_box("README.md")),
                MatchDecision::DeniedByMissingInclude
            );
        });
    });

    group.finish();
}

fn bench_glob_path_depth(c: &mut Criterion) {
    let mut group = c.benchmark_group("glob_path_depth");

    let globs = IncludeExcludeGlobs::new(&patterns(&["**/*.rs"]), &[]).unwrap();

    for depth in [1, 5, 10, 20] {
        let path = (0..depth)
            .map(|i| format!("d{i}"))
            .collect::<Vec<_>>()
            .join("/")
            + "/file.rs";

        group.bench_with_input(BenchmarkId::new("depth", depth), &path, |b, p| {
            b.iter(|| globs.decide_str(black_box(p)));
        });
    }

    group.finish();
}

fn bench_build_globset_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("build_globset");

    for count in [1, 10, 50, 200] {
        let pats: Vec<String> = (0..count).map(|i| format!("path{i}/**/*.ext{i}")).collect();
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(BenchmarkId::new("patterns", count), &pats, |b, p| {
            b.iter(|| build_globset(black_box(p)).unwrap());
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_glob_compilation,
    bench_glob_decide_str,
    bench_glob_match_decision_types,
    bench_glob_path_depth,
    bench_build_globset_scaling,
);
criterion_main!(benches);
