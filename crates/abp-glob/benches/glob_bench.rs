// SPDX-License-Identifier: MIT OR Apache-2.0
use criterion::{Criterion, black_box, criterion_group, criterion_main};

use abp_glob::IncludeExcludeGlobs;

fn patterns(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|x| x.to_string()).collect()
}

// ---------------------------------------------------------------------------
// simple_glob_match — single pattern, single path
// ---------------------------------------------------------------------------

fn bench_simple_glob_match(c: &mut Criterion) {
    let globs = IncludeExcludeGlobs::new(&patterns(&["src/**"]), &patterns(&["*.log"])).unwrap();

    c.bench_function("simple_glob_match", |b| {
        b.iter(|| globs.decide_str(black_box("src/lib.rs")));
    });
}

// ---------------------------------------------------------------------------
// complex_glob_match — 50 patterns, single path
// ---------------------------------------------------------------------------

fn bench_complex_glob_match(c: &mut Criterion) {
    let include: Vec<String> = (0..25).map(|i| format!("dir_{i}/**")).collect();
    let exclude: Vec<String> = (0..25).map(|i| format!("dir_{i}/excluded_*")).collect();
    let globs = IncludeExcludeGlobs::new(&include, &exclude).unwrap();

    c.bench_function("complex_glob_match", |b| {
        b.iter(|| globs.decide_str(black_box("dir_12/foo/bar.rs")));
    });
}

// ---------------------------------------------------------------------------
// many_paths — single pattern, 1000 paths
// ---------------------------------------------------------------------------

fn bench_many_paths(c: &mut Criterion) {
    let globs = IncludeExcludeGlobs::new(&patterns(&["src/**"]), &patterns(&["*.log"])).unwrap();

    let paths: Vec<String> = (0..1000)
        .map(|i| format!("src/module_{}/file_{}.rs", i / 10, i))
        .collect();

    c.bench_function("many_paths", |b| {
        b.iter(|| {
            for p in &paths {
                black_box(globs.decide_str(p));
            }
        });
    });
}

criterion_group!(
    benches,
    bench_simple_glob_match,
    bench_complex_glob_match,
    bench_many_paths,
);
criterion_main!(benches);
