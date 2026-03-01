// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz capability negotiation with random manifests and requirements.
//!
//! Constructs arbitrary [`CapabilityManifest`] and [`CapabilityRequirements`]
//! from fuzzer input and exercises [`negotiate`], [`check_capability`], and
//! [`generate_report`]. Verifies:
//! 1. No panics on any input combination.
//! 2. Result categories are complete: native + emulatable + unsupported == total required.
//! 3. `is_compatible` is consistent with unsupported being empty.
//! 4. Report counts match the negotiation result.
#![no_main]
use abp_capability::{check_capability, generate_report, negotiate};
use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    SupportLevel,
};
use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use std::collections::BTreeMap;

/// All known capability variants for indexed selection.
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

#[derive(Debug, Arbitrary)]
struct CapFuzzInput {
    /// Manifest entries: (capability_index, support_level_index).
    manifest_entries: Vec<(u8, u8)>,
    /// Required capabilities: (capability_index, min_support_index).
    required_caps: Vec<(u8, u8)>,
    /// Extra capability to check individually.
    check_cap_idx: u8,
}

fuzz_target!(|input: CapFuzzInput| {
    // --- Build manifest ---
    let mut manifest: CapabilityManifest = BTreeMap::new();
    for &(cap_idx, level_idx) in &input.manifest_entries {
        let cap = ALL_CAPS[cap_idx as usize % ALL_CAPS.len()].clone();
        let level = match level_idx % 4 {
            0 => SupportLevel::Native,
            1 => SupportLevel::Emulated,
            2 => SupportLevel::Unsupported,
            _ => SupportLevel::Restricted {
                reason: "fuzz-reason".into(),
            },
        };
        manifest.insert(cap, level);
    }

    // --- Build requirements ---
    let requirements = CapabilityRequirements {
        required: input
            .required_caps
            .iter()
            .map(|&(cap_idx, min_idx)| {
                let cap = ALL_CAPS[cap_idx as usize % ALL_CAPS.len()].clone();
                let min = match min_idx % 2 {
                    0 => MinSupport::Native,
                    _ => MinSupport::Emulated,
                };
                CapabilityRequirement {
                    capability: cap,
                    min_support: min,
                }
            })
            .collect(),
    };

    let num_required = requirements.required.len();

    // --- Property 1: negotiate never panics ---
    let result = negotiate(&manifest, &requirements);

    // --- Property 2: categories are complete ---
    assert_eq!(
        result.native.len() + result.emulatable.len() + result.unsupported.len(),
        num_required,
        "native + emulatable + unsupported must equal total required"
    );
    assert_eq!(result.total(), num_required);

    // --- Property 3: is_compatible consistency ---
    assert_eq!(
        result.is_compatible(),
        result.unsupported.is_empty(),
        "is_compatible must match unsupported.is_empty()"
    );

    // --- Property 4: check_capability never panics ---
    let check_cap = ALL_CAPS[input.check_cap_idx as usize % ALL_CAPS.len()].clone();
    let level = check_capability(&manifest, &check_cap);
    // Must return a valid variant.
    let _ = format!("{level:?}");

    // --- Property 5: generate_report never panics & counts match ---
    let report = generate_report(&result);
    assert_eq!(report.compatible, result.is_compatible());
    assert_eq!(report.native_count, result.native.len());
    assert_eq!(report.emulated_count, result.emulatable.len());
    assert_eq!(report.unsupported_count, result.unsupported.len());
    assert!(!report.summary.is_empty());

    // --- Serde round-trips ---
    if let Ok(json) = serde_json::to_string(&result) {
        let rt: abp_capability::NegotiationResult =
            serde_json::from_str(&json).expect("NegotiationResult round-trip must succeed");
        assert_eq!(rt, result);
    }
    if let Ok(json) = serde_json::to_string(&report) {
        let rt: abp_capability::CompatibilityReport =
            serde_json::from_str(&json).expect("CompatibilityReport round-trip must succeed");
        assert_eq!(rt, report);
    }

    // --- Edge case: empty manifest + empty requirements ---
    let empty_result = negotiate(&BTreeMap::new(), &CapabilityRequirements::default());
    assert!(empty_result.is_compatible());
    assert_eq!(empty_result.total(), 0);
});
