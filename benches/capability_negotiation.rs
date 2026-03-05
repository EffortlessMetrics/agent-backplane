// SPDX-License-Identifier: MIT OR Apache-2.0
//! Benchmarks for capability negotiation.

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
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

fn manifest_with(n: usize) -> CapabilityManifest {
    ALL_CAPS
        .iter()
        .take(n)
        .map(|c| (c.clone(), SupportLevel::Native))
        .collect()
}

fn requirements_with(n: usize) -> CapabilityRequirements {
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

// ── negotiate() with varying counts ─────────────────────────────────────

fn bench_negotiate(c: &mut Criterion) {
    let mut group = c.benchmark_group("negotiate");

    for count in [1, 5, 10, 20] {
        let manifest = manifest_with(count);
        let reqs = requirements_with(count);

        group.bench_with_input(
            BenchmarkId::new("capabilities", count),
            &(&manifest, &reqs),
            |b, (m, r)| {
                b.iter(|| negotiate(black_box(m), black_box(r)));
            },
        );
    }

    // Mixed: half native, half missing
    let half = ALL_CAPS.len() / 2;
    let manifest = manifest_with(half);
    let reqs = requirements_with(ALL_CAPS.len());
    group.bench_with_input(
        BenchmarkId::new("mixed", ALL_CAPS.len()),
        &(&manifest, &reqs),
        |b, (m, r)| {
            b.iter(|| negotiate(black_box(m), black_box(r)));
        },
    );

    group.finish();
}

// ── check_capability() single lookup ────────────────────────────────────

fn bench_check_capability(c: &mut Criterion) {
    let mut group = c.benchmark_group("check_capability");

    let full_manifest = manifest_with(ALL_CAPS.len());
    let empty_manifest: CapabilityManifest = BTreeMap::new();

    group.bench_function("native_hit", |b| {
        b.iter(|| check_capability(black_box(&full_manifest), black_box(&Capability::Streaming)));
    });

    group.bench_function("miss", |b| {
        b.iter(|| {
            check_capability(
                black_box(&empty_manifest),
                black_box(&Capability::Streaming),
            )
        });
    });

    group.finish();
}

// ── generate_report() ───────────────────────────────────────────────────

fn bench_generate_report(c: &mut Criterion) {
    let mut group = c.benchmark_group("generate_report");

    for count in [1, 5, 10, 20] {
        let manifest = manifest_with(count);
        let reqs = requirements_with(count);
        let result = negotiate(&manifest, &reqs);

        group.bench_with_input(
            BenchmarkId::new("capabilities", count),
            &result,
            |b, res| {
                b.iter(|| generate_report(black_box(res)));
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_negotiate,
    bench_check_capability,
    bench_generate_report,
);
criterion_main!(benches);
