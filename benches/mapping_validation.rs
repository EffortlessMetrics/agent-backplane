// SPDX-License-Identifier: MIT OR Apache-2.0
//! Benchmarks for cross-dialect mapping validation.

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};

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
                fidelity: Fidelity::Lossless,
            });
            count += 1;
        }
    }
    reg
}

// ── validate_mapping() across dialect pairs ─────────────────────────────

fn bench_validate_mapping(c: &mut Criterion) {
    let mut group = c.benchmark_group("validate_mapping");
    let registry = known_rules();
    let features = all_feature_names();

    let pairs = [
        ("openai_to_claude", Dialect::OpenAi, Dialect::Claude),
        ("claude_to_gemini", Dialect::Claude, Dialect::Gemini),
        ("openai_to_codex", Dialect::OpenAi, Dialect::Codex),
        ("gemini_to_openai", Dialect::Gemini, Dialect::OpenAi),
        ("identity", Dialect::OpenAi, Dialect::OpenAi),
    ];

    for (name, src, tgt) in &pairs {
        group.bench_with_input(
            BenchmarkId::new("pair", name),
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

    group.finish();
}

// ── MappingMatrix construction and lookup ───────────────────────────────

fn bench_mapping_matrix(c: &mut Criterion) {
    let mut group = c.benchmark_group("mapping_matrix");

    let registry = known_rules();

    group.bench_function("from_registry", |b| {
        b.iter(|| MappingMatrix::from_registry(black_box(&registry)));
    });

    let matrix = MappingMatrix::from_registry(&registry);

    group.bench_function("lookup_all_pairs", |b| {
        b.iter(|| {
            for &src in Dialect::all() {
                for &tgt in Dialect::all() {
                    black_box(matrix.is_supported(src, tgt));
                }
            }
        });
    });

    group.finish();
}

// ── MappingRegistry with many rules ─────────────────────────────────────

fn bench_registry_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("registry_operations");

    for rule_count in [10, 36, 100] {
        let reg = registry_with_n_rules(rule_count);

        group.bench_with_input(
            BenchmarkId::new("lookup_existing", rule_count),
            &reg,
            |b, r| {
                b.iter(|| {
                    r.lookup(
                        black_box(Dialect::OpenAi),
                        black_box(Dialect::OpenAi),
                        black_box("feature_0"),
                    )
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("lookup_missing", rule_count),
            &reg,
            |b, r| {
                b.iter(|| {
                    r.lookup(
                        black_box(Dialect::OpenAi),
                        black_box(Dialect::Claude),
                        black_box("nonexistent"),
                    )
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("matrix_from_registry", rule_count),
            &reg,
            |b, r| {
                b.iter(|| MappingMatrix::from_registry(black_box(r)));
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_validate_mapping,
    bench_mapping_matrix,
    bench_registry_operations,
);
criterion_main!(benches);
