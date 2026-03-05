// SPDX-License-Identifier: MIT OR Apache-2.0
//! Benchmark backend selection via the projection matrix.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    SupportLevel, WorkOrderBuilder,
};
use abp_dialect::Dialect;
use abp_mapping::known_rules;
use abp_projection::ProjectionMatrix;

fn make_manifest(caps: &[Capability]) -> CapabilityManifest {
    let mut m = CapabilityManifest::new();
    for cap in caps {
        m.insert(cap.clone(), SupportLevel::Native);
    }
    m
}

fn full_manifest() -> CapabilityManifest {
    make_manifest(&[
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::ToolGlob,
        Capability::ToolGrep,
        Capability::Streaming,
        Capability::ToolUse,
    ])
}

fn partial_manifest() -> CapabilityManifest {
    make_manifest(&[Capability::ToolRead, Capability::Streaming])
}

fn register_backends(matrix: &mut ProjectionMatrix, count: usize) {
    let dialects = Dialect::all();
    for i in 0..count {
        let dialect = dialects[i % dialects.len()];
        let caps = if i % 3 == 0 {
            full_manifest()
        } else {
            partial_manifest()
        };
        matrix.register_backend(format!("backend-{i}"), caps, dialect, (i as u32) % 100);
    }
}

fn bench_matrix_construction(c: &mut Criterion) {
    let mut group = c.benchmark_group("projection_matrix_construction");

    for backend_count in [1, 5, 10, 20] {
        group.bench_with_input(
            BenchmarkId::new("backends", backend_count),
            &backend_count,
            |b, &n| {
                b.iter(|| {
                    let mut m = ProjectionMatrix::new();
                    register_backends(&mut m, n);
                    black_box(m)
                });
            },
        );
    }

    group.finish();
}

fn bench_project_no_requirements(c: &mut Criterion) {
    let mut group = c.benchmark_group("projection_no_requirements");

    for backend_count in [1, 5, 10, 20] {
        let mut matrix = ProjectionMatrix::new();
        register_backends(&mut matrix, backend_count);

        let wo = WorkOrderBuilder::new("Simple task").root("/tmp").build();

        group.bench_with_input(
            BenchmarkId::new("backends", backend_count),
            &(matrix, wo),
            |b, (m, w)| {
                b.iter(|| m.project(black_box(w)).unwrap());
            },
        );
    }

    group.finish();
}

fn bench_project_with_requirements(c: &mut Criterion) {
    let mut group = c.benchmark_group("projection_with_requirements");

    for backend_count in [3, 10, 20] {
        let mut matrix = ProjectionMatrix::new();
        register_backends(&mut matrix, backend_count);

        let mut wo = WorkOrderBuilder::new("Complex task").root("/tmp").build();
        wo.requirements = CapabilityRequirements {
            required: vec![
                CapabilityRequirement {
                    capability: Capability::ToolRead,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::ToolWrite,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::ToolBash,
                    min_support: MinSupport::Emulated,
                },
            ],
        };

        group.bench_with_input(
            BenchmarkId::new("backends", backend_count),
            &(matrix, wo),
            |b, (m, w)| {
                b.iter(|| m.project(black_box(w)).unwrap());
            },
        );
    }

    group.finish();
}

fn bench_project_with_mapping(c: &mut Criterion) {
    let mut group = c.benchmark_group("projection_with_mapping");

    for backend_count in [3, 10, 20] {
        let registry = known_rules();
        let mut matrix = ProjectionMatrix::with_mapping_registry(registry);
        matrix.set_source_dialect(Dialect::OpenAi);
        matrix.set_mapping_features(vec!["tool_use".into(), "streaming".into()]);
        register_backends(&mut matrix, backend_count);

        let wo = WorkOrderBuilder::new("Mapped task").root("/tmp").build();

        group.bench_with_input(
            BenchmarkId::new("backends", backend_count),
            &(matrix, wo),
            |b, (m, w)| {
                b.iter(|| m.project(black_box(w)).unwrap());
            },
        );
    }

    group.finish();
}

fn bench_project_all_dialect_sources(c: &mut Criterion) {
    let mut group = c.benchmark_group("projection_per_dialect");

    let mut matrix = ProjectionMatrix::new();
    register_backends(&mut matrix, 12);
    let wo = WorkOrderBuilder::new("Dialect task").root("/tmp").build();

    for &dialect in Dialect::all() {
        let mut m = matrix.clone();
        m.set_source_dialect(dialect);

        group.bench_with_input(
            BenchmarkId::new("source", dialect.label()),
            &(m, wo.clone()),
            |b, (m, w)| {
                b.iter(|| m.project(black_box(w)).unwrap());
            },
        );
    }

    group.finish();
}

fn bench_fallback_chain_length(c: &mut Criterion) {
    let mut group = c.benchmark_group("projection_fallback_chain");

    for backend_count in [3, 10, 20, 50] {
        let mut matrix = ProjectionMatrix::new();
        register_backends(&mut matrix, backend_count);
        let wo = WorkOrderBuilder::new("Fallback task").root("/tmp").build();

        group.bench_with_input(
            BenchmarkId::new("backends", backend_count),
            &(matrix, wo),
            |b, (m, w)| {
                b.iter(|| {
                    let result = m.project(black_box(w)).unwrap();
                    black_box(result.fallback_chain.len())
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_matrix_construction,
    bench_project_no_requirements,
    bench_project_with_requirements,
    bench_project_with_mapping,
    bench_project_all_dialect_sources,
    bench_fallback_chain_length,
);
criterion_main!(benches);
