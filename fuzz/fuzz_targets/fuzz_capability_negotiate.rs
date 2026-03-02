// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz capability negotiation with random requirements and provided capabilities.
//!
//! Uses raw bytes to construct capability sets and exercises negotiation.
//! Verifies the same invariants as the structured fuzzer but from raw byte input.
#![no_main]
use abp_capability::{check_capability, generate_report, negotiate};
use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    SupportLevel,
};
use libfuzzer_sys::fuzz_target;
use std::collections::BTreeMap;

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

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }

    // Split data: first byte = number of manifest entries, rest alternates.
    let n_manifest = (data[0] as usize) % 32;
    let rest = &data[1..];

    // Build manifest from pairs of bytes.
    let mut manifest: CapabilityManifest = BTreeMap::new();
    for chunk in rest.chunks(2).take(n_manifest) {
        if chunk.len() < 2 {
            break;
        }
        let cap = ALL_CAPS[chunk[0] as usize % ALL_CAPS.len()].clone();
        let level = match chunk[1] % 4 {
            0 => SupportLevel::Native,
            1 => SupportLevel::Emulated,
            2 => SupportLevel::Unsupported,
            _ => SupportLevel::Restricted {
                reason: "fuzz".into(),
            },
        };
        manifest.insert(cap, level);
    }

    // Build requirements from remaining bytes.
    let req_start = 1 + n_manifest * 2;
    let req_data = data.get(req_start..).unwrap_or(&[]);
    let required: Vec<CapabilityRequirement> = req_data
        .chunks(2)
        .take(32)
        .filter_map(|chunk| {
            if chunk.is_empty() {
                return None;
            }
            let cap = ALL_CAPS[chunk[0] as usize % ALL_CAPS.len()].clone();
            let min = if chunk.len() > 1 && chunk[1] % 2 == 0 {
                MinSupport::Native
            } else {
                MinSupport::Emulated
            };
            Some(CapabilityRequirement {
                capability: cap,
                min_support: min,
            })
        })
        .collect();

    let requirements = CapabilityRequirements { required };
    let num_required = requirements.required.len();

    // --- negotiate never panics ---
    let result = negotiate(&manifest, &requirements);

    // --- categories complete ---
    assert_eq!(
        result.native.len() + result.emulatable.len() + result.unsupported.len(),
        num_required,
    );

    // --- is_compatible consistency ---
    assert_eq!(result.is_compatible(), result.unsupported.is_empty());

    // --- check_capability never panics ---
    let check_idx = data.last().copied().unwrap_or(0) as usize % ALL_CAPS.len();
    let _ = check_capability(&manifest, &ALL_CAPS[check_idx]);

    // --- generate_report never panics ---
    let report = generate_report(&result);
    assert_eq!(report.compatible, result.is_compatible());
    assert!(!report.summary.is_empty());
});
