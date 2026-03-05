// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive benchmarks for capability negotiation with various
//! requirement sets, manifest sizes, and edge cases.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use std::collections::BTreeMap;

use abp_capability::{check_capability, generate_report, negotiate};
use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    SupportLevel,
};

// ── Helpers ─────────────────────────────────────────────────────────────

const ALL_CAPS: &[Capability] = &[
    Capability::Streaming,
    Capability::ToolRead,
    Capability::ToolWrite,
    Capability::ToolEdit,
    Capability::ToolBash,
    Capability::ToolGlob,
    Capability::ToolGrep,
    Capability::ToolWebSearch,
    Capability::ToolWebFetch,
    Capability::ToolAskUser,
    Capability::HooksPreToolUse,
    Capability::HooksPostToolUse,
    Capability::SessionResume,
    Capability::SessionFork,
    Capability::Checkpointing,
    Capability::StructuredOutputJsonSchema,
    Capability::McpClient,
    Capability::McpServer,
    Capability::ToolUse,
    Capability::ExtendedThinking,
    Capability::ImageInput,
    Capability::PdfInput,
    Capability::CodeExecution,
    Capability::Logprobs,
    Capability::SeedDeterminism,
    Capability::StopSequences,
];

fn manifest_native(n: usize) -> CapabilityManifest {
    ALL_CAPS
        .iter()
        .take(n)
        .map(|c| (c.clone(), SupportLevel::Native))
        .collect()
}

fn manifest_emulated(n: usize) -> CapabilityManifest {
    ALL_CAPS
        .iter()
        .take(n)
        .map(|c| (c.clone(), SupportLevel::Emulated))
        .collect()
}

fn manifest_mixed(n: usize) -> CapabilityManifest {
    ALL_CAPS
        .iter()
        .take(n)
        .enumerate()
        .map(|(i, c)| {
            let level = match i % 3 {
                0 => SupportLevel::Native,
                1 => SupportLevel::Emulated,
                _ => SupportLevel::Unsupported,
            };
            (c.clone(), level)
        })
        .collect()
}

fn requirements_native(n: usize) -> CapabilityRequirements {
    CapabilityRequirements {
        required: ALL_CAPS
            .iter()
            .take(n)
            .map(|c| CapabilityRequirement {
                capability: c.clone(),
                min_support: MinSupport::Native,
            })
            .collect(),
    }
}

fn requirements_emulated(n: usize) -> CapabilityRequirements {
    CapabilityRequirements {
        required: ALL_CAPS
            .iter()
            .take(n)
            .map(|c| CapabilityRequirement {
                capability: c.clone(),
                min_support: MinSupport::Emulated,
            })
            .collect(),
    }
}

// ── negotiate() scaling ─────────────────────────────────────────────────

fn bench_negotiate(c: &mut Criterion) {
    let mut group = c.benchmark_group("negotiate");

    for count in [1, 5, 10, 20, ALL_CAPS.len()] {
        let manifest = manifest_native(count);
        let reqs = requirements_native(count);

        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(
            BenchmarkId::new("all_native", count),
            &(&manifest, &reqs),
            |b, (m, r)| {
                b.iter(|| negotiate(black_box(m), black_box(r)));
            },
        );
    }

    // Native manifest vs emulated requirements (all should pass)
    for count in [5, 10, 20] {
        let manifest = manifest_native(count);
        let reqs = requirements_emulated(count);

        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(
            BenchmarkId::new("native_vs_emulated_req", count),
            &(&manifest, &reqs),
            |b, (m, r)| {
                b.iter(|| negotiate(black_box(m), black_box(r)));
            },
        );
    }

    // Emulated manifest vs native requirements (all should fail)
    for count in [5, 10, 20] {
        let manifest = manifest_emulated(count);
        let reqs = requirements_native(count);

        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(
            BenchmarkId::new("emulated_vs_native_req", count),
            &(&manifest, &reqs),
            |b, (m, r)| {
                b.iter(|| negotiate(black_box(m), black_box(r)));
            },
        );
    }

    // Mixed manifest — realistic scenario
    let half = ALL_CAPS.len() / 2;
    let manifest = manifest_mixed(ALL_CAPS.len());
    let reqs = requirements_emulated(half);
    group.throughput(Throughput::Elements(half as u64));
    group.bench_with_input(
        BenchmarkId::new("mixed_manifest", half),
        &(&manifest, &reqs),
        |b, (m, r)| {
            b.iter(|| negotiate(black_box(m), black_box(r)));
        },
    );

    // Empty manifest vs full requirements (all miss)
    let empty: CapabilityManifest = BTreeMap::new();
    let full_reqs = requirements_native(ALL_CAPS.len());
    group.throughput(Throughput::Elements(ALL_CAPS.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("empty_manifest", ALL_CAPS.len()),
        &(&empty, &full_reqs),
        |b, (m, r)| {
            b.iter(|| negotiate(black_box(m), black_box(r)));
        },
    );

    group.finish();
}

// ── check_capability() ──────────────────────────────────────────────────

fn bench_check_capability(c: &mut Criterion) {
    let mut group = c.benchmark_group("check_capability");

    let full = manifest_native(ALL_CAPS.len());
    let empty: CapabilityManifest = BTreeMap::new();
    let small = manifest_native(3);

    // Hit in full manifest (BTreeMap lookup)
    group.bench_function("native_hit_full", |b| {
        b.iter(|| check_capability(black_box(&full), black_box(&Capability::Streaming)));
    });

    // Miss in empty manifest
    group.bench_function("miss_empty", |b| {
        b.iter(|| check_capability(black_box(&empty), black_box(&Capability::Streaming)));
    });

    // Hit in small manifest
    group.bench_function("native_hit_small", |b| {
        b.iter(|| check_capability(black_box(&small), black_box(&Capability::Streaming)));
    });

    // Miss in full manifest (capability not present but manifest is large)
    // Use last capability and small manifest that doesn't include it
    group.bench_function("miss_in_small", |b| {
        b.iter(|| check_capability(black_box(&small), black_box(&Capability::StopSequences)));
    });

    group.finish();
}

// ── generate_report() ───────────────────────────────────────────────────

fn bench_generate_report(c: &mut Criterion) {
    let mut group = c.benchmark_group("generate_report");

    // All pass
    for count in [1, 5, 10, 20] {
        let manifest = manifest_native(count);
        let reqs = requirements_native(count);
        let result = negotiate(&manifest, &reqs);

        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(BenchmarkId::new("all_pass", count), &result, |b, res| {
            b.iter(|| generate_report(black_box(res)));
        });
    }

    // All fail
    for count in [5, 10, 20] {
        let empty: CapabilityManifest = BTreeMap::new();
        let reqs = requirements_native(count);
        let result = negotiate(&empty, &reqs);

        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(BenchmarkId::new("all_fail", count), &result, |b, res| {
            b.iter(|| generate_report(black_box(res)));
        });
    }

    // Mixed results
    let manifest = manifest_mixed(ALL_CAPS.len());
    let reqs = requirements_native(ALL_CAPS.len());
    let result = negotiate(&manifest, &reqs);
    group.throughput(Throughput::Elements(ALL_CAPS.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("mixed", ALL_CAPS.len()),
        &result,
        |b, res| {
            b.iter(|| generate_report(black_box(res)));
        },
    );

    group.finish();
}

// ── Full pipeline: negotiate → report ───────────────────────────────────

fn bench_full_pipeline(c: &mut Criterion) {
    let manifest = manifest_native(ALL_CAPS.len());
    let reqs = requirements_native(ALL_CAPS.len());

    c.bench_function("capability_full_pipeline", |b| {
        b.iter(|| {
            let result = negotiate(black_box(&manifest), black_box(&reqs));
            generate_report(black_box(&result))
        });
    });
}

criterion_group!(
    benches,
    bench_negotiate,
    bench_check_capability,
    bench_generate_report,
    bench_full_pipeline,
);
criterion_main!(benches);
