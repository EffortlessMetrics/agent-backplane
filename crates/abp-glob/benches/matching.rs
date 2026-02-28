use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};

use abp_glob::IncludeExcludeGlobs;

fn patterns(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|x| x.to_string()).collect()
}

fn bench_compilation(c: &mut Criterion) {
    let mut group = c.benchmark_group("glob_compile");

    let single_include = patterns(&["src/**"]);
    let single_exclude = patterns(&["target/**"]);
    group.bench_function("single_pattern", |b| {
        b.iter(|| {
            IncludeExcludeGlobs::new(black_box(&single_include), black_box(&single_exclude))
                .unwrap()
        })
    });

    let many_include = patterns(&["src/**", "tests/**", "benches/**", "examples/**", "docs/**"]);
    let many_exclude = patterns(&[
        "target/**",
        "*.log",
        "*.tmp",
        "**/.git/**",
        "**/node_modules/**",
        "**/.env",
        "**/.env.*",
        "**/secret*",
    ]);
    group.bench_function("many_patterns", |b| {
        b.iter(|| {
            IncludeExcludeGlobs::new(black_box(&many_include), black_box(&many_exclude)).unwrap()
        })
    });

    group.finish();
}

fn bench_decide_str(c: &mut Criterion) {
    let include = patterns(&["src/**", "tests/**", "benches/**", "examples/**", "docs/**"]);
    let exclude = patterns(&[
        "target/**",
        "*.log",
        "*.tmp",
        "**/.git/**",
        "**/node_modules/**",
        "**/.env",
        "**/.env.*",
        "**/secret*",
    ]);
    let globs = IncludeExcludeGlobs::new(&include, &exclude).unwrap();

    let test_paths = [
        "src/lib.rs",
        "src/a/b/c/deep/nested/file.rs",
        "tests/integration/test_auth.rs",
        "target/debug/build/something",
        "README.md",
        ".env",
        "node_modules/lodash/index.js",
        "docs/guide.md",
        "secret_keys.txt",
        "src/generated/output.rs",
    ];

    let mut group = c.benchmark_group("decide_str");
    for path in &test_paths {
        group.bench_with_input(BenchmarkId::from_parameter(path), path, |b, p| {
            b.iter(|| globs.decide_str(black_box(p)))
        });
    }
    group.finish();
}

fn bench_single_vs_many_patterns(c: &mut Criterion) {
    let single = IncludeExcludeGlobs::new(&patterns(&["src/**"]), &patterns(&["*.log"])).unwrap();

    let many = IncludeExcludeGlobs::new(
        &patterns(&[
            "src/**",
            "tests/**",
            "benches/**",
            "examples/**",
            "docs/**",
            "crates/**",
            "lib/**",
            "bin/**",
        ]),
        &patterns(&[
            "target/**",
            "*.log",
            "*.tmp",
            "**/.git/**",
            "**/node_modules/**",
            "**/.env",
            "**/.env.*",
            "**/secret*",
        ]),
    )
    .unwrap();

    let path = "src/a/b/c/module.rs";

    let mut group = c.benchmark_group("pattern_count");
    group.bench_function("single_pattern", |b| {
        b.iter(|| single.decide_str(black_box(path)))
    });
    group.bench_function("many_patterns", |b| {
        b.iter(|| many.decide_str(black_box(path)))
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_compilation,
    bench_decide_str,
    bench_single_vs_many_patterns,
);
criterion_main!(benches);
