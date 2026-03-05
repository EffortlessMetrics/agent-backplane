// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive benchmarks for IR conversion, mapping lookup, matrix queries,
//! and cross-dialect validation.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};

use abp_dialect::Dialect;
use abp_mapping::{
    Fidelity, MappingMatrix, MappingRegistry, MappingRule, features, known_rules, validate_mapping,
};

// ── Helpers ─────────────────────────────────────────────────────────────

fn all_feature_names() -> Vec<String> {
    vec![
        features::TOOL_USE.into(),
        features::STREAMING.into(),
        features::THINKING.into(),
        features::IMAGE_INPUT.into(),
    ]
}

fn registry_with_n_rules(n: usize) -> MappingRegistry {
    let mut reg = MappingRegistry::new();
    let dialects = Dialect::all();
    let mut count = 0;
    for &src in dialects {
        for &tgt in dialects {
            if count >= n {
                return reg;
            }
            reg.insert(MappingRule {
                source_dialect: src,
                target_dialect: tgt,
                feature: format!("feature_{count}"),
                fidelity: if count % 3 == 0 {
                    Fidelity::Lossless
                } else if count % 3 == 1 {
                    Fidelity::LossyLabeled {
                        warning: format!("lossy mapping {count}"),
                    }
                } else {
                    Fidelity::Unsupported {
                        reason: format!("unsupported {count}"),
                    }
                },
            });
            count += 1;
        }
    }
    reg
}

fn many_features(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("feature_{i}")).collect()
}

// ── Registry construction ───────────────────────────────────────────────

fn bench_registry_construction(c: &mut Criterion) {
    let mut group = c.benchmark_group("registry_construction");

    group.bench_function("known_rules", |b| {
        b.iter(|| black_box(known_rules()));
    });

    for n in [10, 50, 100, 500] {
        group.bench_with_input(BenchmarkId::new("synthetic_rules", n), &n, |b, &count| {
            b.iter(|| registry_with_n_rules(black_box(count)));
        });
    }

    group.finish();
}

// ── Registry lookup ─────────────────────────────────────────────────────

fn bench_registry_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("registry_lookup");

    for rule_count in [10, 50, 100, 500] {
        let reg = registry_with_n_rules(rule_count);

        group.bench_with_input(BenchmarkId::new("hit", rule_count), &reg, |b, r| {
            b.iter(|| {
                r.lookup(
                    black_box(Dialect::OpenAi),
                    black_box(Dialect::OpenAi),
                    black_box("feature_0"),
                )
            });
        });

        group.bench_with_input(BenchmarkId::new("miss", rule_count), &reg, |b, r| {
            b.iter(|| {
                r.lookup(
                    black_box(Dialect::OpenAi),
                    black_box(Dialect::Claude),
                    black_box("nonexistent_feature"),
                )
            });
        });

        // Scan all rules
        group.bench_with_input(BenchmarkId::new("iterate_all", rule_count), &reg, |b, r| {
            b.iter(|| {
                let count = r.iter().filter(|rule| rule.fidelity.is_lossless()).count();
                black_box(count);
            });
        });
    }

    group.finish();
}

// ── MappingMatrix ───────────────────────────────────────────────────────

fn bench_matrix_construction(c: &mut Criterion) {
    let mut group = c.benchmark_group("matrix_construction");

    for rule_count in [10, 50, 100, 500] {
        let reg = registry_with_n_rules(rule_count);
        group.bench_with_input(
            BenchmarkId::new("from_registry", rule_count),
            &reg,
            |b, r| {
                b.iter(|| MappingMatrix::from_registry(black_box(r)));
            },
        );
    }

    group.finish();
}

fn bench_matrix_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("matrix_lookup");
    let matrix = MappingMatrix::from_registry(&known_rules());

    // Single lookup
    group.bench_function("single_pair", |b| {
        b.iter(|| matrix.is_supported(black_box(Dialect::OpenAi), black_box(Dialect::Claude)));
    });

    // All pairs exhaustive lookup
    let dialects = Dialect::all();
    let pair_count = dialects.len() * dialects.len();
    group.throughput(Throughput::Elements(pair_count as u64));
    group.bench_function("all_pairs", |b| {
        b.iter(|| {
            for &src in dialects {
                for &tgt in dialects {
                    black_box(matrix.is_supported(src, tgt));
                }
            }
        });
    });

    group.finish();
}

// ── validate_mapping with varying feature counts ────────────────────────

fn bench_validate_mapping(c: &mut Criterion) {
    let mut group = c.benchmark_group("validate_mapping");
    let registry = known_rules();

    let pairs = [
        ("openai_to_claude", Dialect::OpenAi, Dialect::Claude),
        ("claude_to_gemini", Dialect::Claude, Dialect::Gemini),
        ("identity", Dialect::OpenAi, Dialect::OpenAi),
    ];

    let features = all_feature_names();
    for (name, src, tgt) in &pairs {
        group.throughput(Throughput::Elements(features.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("known_features", name),
            &(&registry, &features),
            |b, (reg, feats)| {
                b.iter(|| {
                    validate_mapping(
                        black_box(reg),
                        black_box(*src),
                        black_box(*tgt),
                        black_box(feats),
                    )
                });
            },
        );
    }

    // Scaling: many synthetic features against a large registry
    for feat_count in [10, 50, 100] {
        let large_reg = registry_with_n_rules(500);
        let feats = many_features(feat_count);
        group.throughput(Throughput::Elements(feat_count as u64));
        group.bench_with_input(
            BenchmarkId::new("synthetic_features", feat_count),
            &(&large_reg, &feats),
            |b, (reg, fs)| {
                b.iter(|| {
                    validate_mapping(
                        black_box(reg),
                        black_box(Dialect::OpenAi),
                        black_box(Dialect::Claude),
                        black_box(fs),
                    )
                });
            },
        );
    }

    group.finish();
}

// ── Full pipeline: registry → matrix → validate ─────────────────────────

fn bench_full_pipeline(c: &mut Criterion) {
    let features = all_feature_names();

    c.bench_function("mapping_full_pipeline", |b| {
        b.iter(|| {
            let reg = known_rules();
            let _matrix = MappingMatrix::from_registry(&reg);
            let _results =
                validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, black_box(&features));
        });
    });
}

criterion_group!(
    benches,
    bench_registry_construction,
    bench_registry_lookup,
    bench_matrix_construction,
    bench_matrix_lookup,
    bench_validate_mapping,
    bench_full_pipeline,
);
criterion_main!(benches);
