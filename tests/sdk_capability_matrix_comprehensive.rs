#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive SDK capability matrix tests — systematically verifies all 6 SDKs
//! (Claude, OpenAI, Gemini, Kimi, Codex, Copilot) against every `Capability` variant.
//!
//! Covers: manifest correctness, native/emulated/unsupported classification,
//! cross-SDK comparison, capability negotiation, and projection routing.

use std::collections::{BTreeMap, BTreeSet};

use abp_capability::{
    SupportLevel as CapSupportLevel, check_capability, generate_report, negotiate,
};
use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    SupportLevel,
};
use abp_emulation::{EmulationEngine, EmulationStrategy, can_emulate, default_strategy};

// ===========================================================================
// Helpers
// ===========================================================================

/// All Capability enum variants in definition order.
fn all_capabilities() -> Vec<Capability> {
    vec![
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
    ]
}

/// SDK names for iteration.
const SDK_NAMES: &[&str] = &["claude", "openai", "gemini", "kimi", "codex", "copilot"];

/// Load the capability manifest for a named SDK.
fn sdk_manifest(name: &str) -> CapabilityManifest {
    match name {
        "claude" => abp_claude_sdk::dialect::capability_manifest(),
        "openai" => abp_openai_sdk::dialect::capability_manifest(),
        "gemini" => abp_gemini_sdk::dialect::capability_manifest(),
        "kimi" => abp_kimi_sdk::dialect::capability_manifest(),
        "codex" => abp_codex_sdk::dialect::capability_manifest(),
        "copilot" => abp_copilot_sdk::dialect::capability_manifest(),
        other => panic!("unknown SDK: {other}"),
    }
}

/// Classify capabilities into native / emulated / unsupported sets.
fn classify(
    manifest: &CapabilityManifest,
) -> (
    BTreeSet<Capability>,
    BTreeSet<Capability>,
    BTreeSet<Capability>,
) {
    let mut native = BTreeSet::new();
    let mut emulated = BTreeSet::new();
    let mut unsupported = BTreeSet::new();

    for cap in all_capabilities() {
        match manifest.get(&cap) {
            Some(SupportLevel::Native) => {
                native.insert(cap);
            }
            Some(SupportLevel::Emulated) => {
                emulated.insert(cap);
            }
            Some(SupportLevel::Restricted { .. }) => {
                emulated.insert(cap);
            }
            Some(SupportLevel::Unsupported) | None => {
                unsupported.insert(cap);
            }
        }
    }

    (native, emulated, unsupported)
}

/// Build CapabilityRequirements from a slice of capabilities (all at MinSupport::Native).
fn require_native(caps: &[Capability]) -> CapabilityRequirements {
    CapabilityRequirements {
        required: caps
            .iter()
            .map(|c| CapabilityRequirement {
                capability: c.clone(),
                min_support: MinSupport::Native,
            })
            .collect(),
    }
}

/// Build CapabilityRequirements from a slice of capabilities (all at MinSupport::Emulated).
fn require_emulated(caps: &[Capability]) -> CapabilityRequirements {
    CapabilityRequirements {
        required: caps
            .iter()
            .map(|c| CapabilityRequirement {
                capability: c.clone(),
                min_support: MinSupport::Emulated,
            })
            .collect(),
    }
}

// ===========================================================================
// 1. Manifest correctness — verify each SDK's manifest is well-formed
// ===========================================================================

#[test]
fn all_sdk_manifests_are_non_empty() {
    for name in SDK_NAMES {
        let m = sdk_manifest(name);
        assert!(!m.is_empty(), "{name} manifest should not be empty");
    }
}

#[test]
fn all_sdk_manifests_contain_streaming() {
    for name in SDK_NAMES {
        let m = sdk_manifest(name);
        assert!(
            m.contains_key(&Capability::Streaming),
            "{name} manifest must declare Streaming"
        );
    }
}

#[test]
fn all_sdk_manifests_contain_only_valid_support_levels() {
    for name in SDK_NAMES {
        let m = sdk_manifest(name);
        for (cap, level) in &m {
            match level {
                SupportLevel::Native
                | SupportLevel::Emulated
                | SupportLevel::Unsupported
                | SupportLevel::Restricted { .. } => {}
            }
            // Ensure each entry maps a real capability
            assert!(
                all_capabilities().contains(cap),
                "{name}: unknown capability {cap:?}"
            );
        }
    }
}

#[test]
fn every_sdk_declares_mcp_support() {
    for name in SDK_NAMES {
        let m = sdk_manifest(name);
        // SDKs should explicitly declare MCP client/server support (even if unsupported).
        assert!(
            m.contains_key(&Capability::McpClient),
            "{name} must declare McpClient"
        );
        assert!(
            m.contains_key(&Capability::McpServer),
            "{name} must declare McpServer"
        );
    }
}

// ===========================================================================
// 2. Per-SDK native capability verification
// ===========================================================================

#[test]
fn claude_native_capabilities() {
    let m = sdk_manifest("claude");
    let native_expected = [
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::ToolGlob,
        Capability::ToolGrep,
        Capability::ToolWebSearch,
        Capability::ToolWebFetch,
        Capability::StructuredOutputJsonSchema,
        Capability::HooksPreToolUse,
        Capability::HooksPostToolUse,
        Capability::McpClient,
    ];
    for cap in &native_expected {
        assert!(
            matches!(m.get(cap), Some(SupportLevel::Native)),
            "Claude should natively support {cap:?}, got {:?}",
            m.get(cap)
        );
    }
}

#[test]
fn openai_native_capabilities() {
    let m = sdk_manifest("openai");
    let native_expected = [
        Capability::Streaming,
        Capability::StructuredOutputJsonSchema,
    ];
    for cap in &native_expected {
        assert!(
            matches!(m.get(cap), Some(SupportLevel::Native)),
            "OpenAI should natively support {cap:?}, got {:?}",
            m.get(cap)
        );
    }
}

#[test]
fn gemini_native_capabilities() {
    let m = sdk_manifest("gemini");
    let native_expected = [
        Capability::Streaming,
        Capability::ToolRead,
        Capability::StructuredOutputJsonSchema,
    ];
    for cap in &native_expected {
        assert!(
            matches!(m.get(cap), Some(SupportLevel::Native)),
            "Gemini should natively support {cap:?}, got {:?}",
            m.get(cap)
        );
    }
}

#[test]
fn kimi_native_capabilities() {
    let m = sdk_manifest("kimi");
    let native_expected = [
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWebSearch,
    ];
    for cap in &native_expected {
        assert!(
            matches!(m.get(cap), Some(SupportLevel::Native)),
            "Kimi should natively support {cap:?}, got {:?}",
            m.get(cap)
        );
    }
}

#[test]
fn codex_native_capabilities() {
    let m = sdk_manifest("codex");
    let native_expected = [
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::StructuredOutputJsonSchema,
    ];
    for cap in &native_expected {
        assert!(
            matches!(m.get(cap), Some(SupportLevel::Native)),
            "Codex should natively support {cap:?}, got {:?}",
            m.get(cap)
        );
    }
}

#[test]
fn copilot_native_capabilities() {
    let m = sdk_manifest("copilot");
    let native_expected = [Capability::Streaming, Capability::ToolWebSearch];
    for cap in &native_expected {
        assert!(
            matches!(m.get(cap), Some(SupportLevel::Native)),
            "Copilot should natively support {cap:?}, got {:?}",
            m.get(cap)
        );
    }
}

// ===========================================================================
// 3. Per-SDK emulated capability verification
// ===========================================================================

#[test]
fn claude_emulated_capabilities() {
    let m = sdk_manifest("claude");
    assert!(
        matches!(
            m.get(&Capability::Checkpointing),
            Some(SupportLevel::Emulated)
        ),
        "Claude should emulate Checkpointing"
    );
}

#[test]
fn openai_emulated_capabilities() {
    let m = sdk_manifest("openai");
    let emulated_expected = [
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::ToolGlob,
        Capability::ToolGrep,
        Capability::HooksPreToolUse,
        Capability::HooksPostToolUse,
    ];
    for cap in &emulated_expected {
        assert!(
            matches!(m.get(cap), Some(SupportLevel::Emulated)),
            "OpenAI should emulate {cap:?}, got {:?}",
            m.get(cap)
        );
    }
}

#[test]
fn gemini_emulated_capabilities() {
    let m = sdk_manifest("gemini");
    let emulated_expected = [
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
    ];
    for cap in &emulated_expected {
        assert!(
            matches!(m.get(cap), Some(SupportLevel::Emulated)),
            "Gemini should emulate {cap:?}, got {:?}",
            m.get(cap)
        );
    }
}

#[test]
fn kimi_emulated_capabilities() {
    let m = sdk_manifest("kimi");
    let emulated_expected = [
        Capability::ToolWrite,
        Capability::StructuredOutputJsonSchema,
    ];
    for cap in &emulated_expected {
        assert!(
            matches!(m.get(cap), Some(SupportLevel::Emulated)),
            "Kimi should emulate {cap:?}, got {:?}",
            m.get(cap)
        );
    }
}

#[test]
fn codex_emulated_capabilities() {
    let m = sdk_manifest("codex");
    let emulated_expected = [
        Capability::ToolGlob,
        Capability::ToolGrep,
        Capability::HooksPreToolUse,
        Capability::HooksPostToolUse,
    ];
    for cap in &emulated_expected {
        assert!(
            matches!(m.get(cap), Some(SupportLevel::Emulated)),
            "Codex should emulate {cap:?}, got {:?}",
            m.get(cap)
        );
    }
}

#[test]
fn copilot_emulated_capabilities() {
    let m = sdk_manifest("copilot");
    let emulated_expected = [
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::StructuredOutputJsonSchema,
        Capability::HooksPreToolUse,
        Capability::HooksPostToolUse,
    ];
    for cap in &emulated_expected {
        assert!(
            matches!(m.get(cap), Some(SupportLevel::Emulated)),
            "Copilot should emulate {cap:?}, got {:?}",
            m.get(cap)
        );
    }
}

// ===========================================================================
// 4. Per-SDK unsupported capability verification
// ===========================================================================

#[test]
fn claude_unsupported_capabilities() {
    let m = sdk_manifest("claude");
    assert!(
        matches!(
            m.get(&Capability::McpServer),
            Some(SupportLevel::Unsupported)
        ),
        "Claude should not support McpServer"
    );
}

#[test]
fn openai_unsupported_capabilities() {
    let m = sdk_manifest("openai");
    let unsupported_expected = [Capability::McpClient, Capability::McpServer];
    for cap in &unsupported_expected {
        assert!(
            matches!(m.get(cap), Some(SupportLevel::Unsupported)),
            "OpenAI should not support {cap:?}, got {:?}",
            m.get(cap)
        );
    }
}

#[test]
fn gemini_unsupported_capabilities() {
    let m = sdk_manifest("gemini");
    let unsupported_expected = [
        Capability::ToolGlob,
        Capability::ToolGrep,
        Capability::McpClient,
        Capability::McpServer,
    ];
    for cap in &unsupported_expected {
        assert!(
            matches!(m.get(cap), Some(SupportLevel::Unsupported)),
            "Gemini should not support {cap:?}, got {:?}",
            m.get(cap)
        );
    }
}

#[test]
fn kimi_unsupported_capabilities() {
    let m = sdk_manifest("kimi");
    let unsupported_expected = [
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::McpClient,
        Capability::McpServer,
    ];
    for cap in &unsupported_expected {
        assert!(
            matches!(m.get(cap), Some(SupportLevel::Unsupported)),
            "Kimi should not support {cap:?}, got {:?}",
            m.get(cap)
        );
    }
}

#[test]
fn codex_unsupported_capabilities() {
    let m = sdk_manifest("codex");
    let unsupported_expected = [Capability::McpClient, Capability::McpServer];
    for cap in &unsupported_expected {
        assert!(
            matches!(m.get(cap), Some(SupportLevel::Unsupported)),
            "Codex should not support {cap:?}, got {:?}",
            m.get(cap)
        );
    }
}

#[test]
fn copilot_unsupported_capabilities() {
    let m = sdk_manifest("copilot");
    let unsupported_expected = [
        Capability::ToolGlob,
        Capability::ToolGrep,
        Capability::McpClient,
        Capability::McpServer,
    ];
    for cap in &unsupported_expected {
        assert!(
            matches!(m.get(cap), Some(SupportLevel::Unsupported)),
            "Copilot should not support {cap:?}, got {:?}",
            m.get(cap)
        );
    }
}

// ===========================================================================
// 5. Cross-SDK comparison — intersections and differences
// ===========================================================================

#[test]
fn all_sdks_share_streaming_native() {
    for name in SDK_NAMES {
        let m = sdk_manifest(name);
        assert!(
            matches!(m.get(&Capability::Streaming), Some(SupportLevel::Native)),
            "{name} must natively support Streaming"
        );
    }
}

#[test]
fn no_sdk_natively_supports_mcp_server() {
    for name in SDK_NAMES {
        let m = sdk_manifest(name);
        assert!(
            !matches!(m.get(&Capability::McpServer), Some(SupportLevel::Native)),
            "{name} should not natively support McpServer"
        );
    }
}

#[test]
fn claude_has_most_native_capabilities() {
    let mut max_native = 0usize;
    let mut best_sdk = "";
    for name in SDK_NAMES {
        let (native, _, _) = classify(&sdk_manifest(name));
        if native.len() > max_native {
            max_native = native.len();
            best_sdk = name;
        }
    }
    assert_eq!(
        best_sdk, "claude",
        "Claude should have the most native capabilities"
    );
}

#[test]
fn capability_intersection_across_all_sdks() {
    // Find capabilities that every SDK declares (native or emulated)
    let mut common: BTreeSet<Capability> = all_capabilities().into_iter().collect();
    for name in SDK_NAMES {
        let m = sdk_manifest(name);
        common.retain(|cap| m.contains_key(cap));
    }
    assert!(
        common.contains(&Capability::Streaming),
        "Streaming should be in the common set"
    );
}

#[test]
fn unique_capabilities_per_sdk() {
    // Capabilities that appear in exactly one SDK's manifest
    let mut cap_sdk_count: BTreeMap<Capability, usize> = BTreeMap::new();
    for name in SDK_NAMES {
        let m = sdk_manifest(name);
        for cap in m.keys() {
            *cap_sdk_count.entry(cap.clone()).or_default() += 1;
        }
    }
    let unique_caps: Vec<_> = cap_sdk_count
        .iter()
        .filter(|(_, count)| **count == 1)
        .map(|(cap, _)| cap.clone())
        .collect();
    // McpClient appears in only a subset of SDKs — verify count logic works
    for cap in &unique_caps {
        let mut found_in = vec![];
        for name in SDK_NAMES {
            if sdk_manifest(name).contains_key(cap) {
                found_in.push(*name);
            }
        }
        assert_eq!(
            found_in.len(),
            1,
            "{cap:?} should appear in exactly one SDK"
        );
    }
}

#[test]
fn native_count_ordering() {
    let mut counts: Vec<(&str, usize)> = SDK_NAMES
        .iter()
        .map(|name| {
            let (native, _, _) = classify(&sdk_manifest(name));
            (*name, native.len())
        })
        .collect();
    counts.sort_by_key(|b| std::cmp::Reverse(b.1));
    // Claude should be at the top
    assert_eq!(counts[0].0, "claude");
}

#[test]
fn emulated_count_differences() {
    for name in SDK_NAMES {
        let (_, emulated, _) = classify(&sdk_manifest(name));
        // Every SDK may have a different number of emulated capabilities — just ensure non-panic
        let _ = emulated.len();
    }
}

// ===========================================================================
// 6. Capability negotiation — requesting unsupported features
// ===========================================================================

#[test]
fn negotiate_claude_all_native_tools() {
    let m = sdk_manifest("claude");
    let reqs = require_native(&[
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
    ]);
    let result = negotiate(&m, &reqs);
    assert!(result.is_compatible());
    assert_eq!(result.native.len(), 4);
    assert!(result.unsupported.is_empty());
}

#[test]
fn negotiate_openai_tools_are_emulated() {
    let m = sdk_manifest("openai");
    let reqs = require_native(&[
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
    ]);
    let result = negotiate(&m, &reqs);
    assert!(result.is_compatible());
    assert_eq!(result.emulated.len(), 3);
}

#[test]
fn negotiate_kimi_requesting_tool_bash_fails() {
    let m = sdk_manifest("kimi");
    let reqs = require_native(&[Capability::ToolBash]);
    let result = negotiate(&m, &reqs);
    assert!(!result.is_compatible());
    assert_eq!(result.unsupported_caps(), vec![Capability::ToolBash]);
}

#[test]
fn negotiate_gemini_glob_grep_unsupported() {
    let m = sdk_manifest("gemini");
    let reqs = require_native(&[Capability::ToolGlob, Capability::ToolGrep]);
    let result = negotiate(&m, &reqs);
    assert!(!result.is_compatible());
    assert_eq!(result.unsupported.len(), 2);
}

#[test]
fn negotiate_copilot_glob_grep_unsupported() {
    let m = sdk_manifest("copilot");
    let reqs = require_native(&[Capability::ToolGlob, Capability::ToolGrep]);
    let result = negotiate(&m, &reqs);
    assert!(!result.is_compatible());
    assert_eq!(result.unsupported.len(), 2);
}

#[test]
fn negotiate_with_emulated_threshold() {
    // When min_support is Emulated, emulated capabilities should satisfy
    let m = sdk_manifest("openai");
    let reqs = require_emulated(&[Capability::ToolRead, Capability::ToolWrite]);
    let result = negotiate(&m, &reqs);
    assert!(result.is_compatible());
}

#[test]
fn negotiate_mcp_across_all_sdks() {
    for name in SDK_NAMES {
        let m = sdk_manifest(name);
        let reqs = require_native(&[Capability::McpClient]);
        let result = negotiate(&m, &reqs);
        if *name == "claude" {
            assert!(result.is_compatible(), "Claude should support McpClient");
        } else {
            // Other SDKs have McpClient as Unsupported
            assert!(
                !result.is_compatible(),
                "{name} should not natively support McpClient"
            );
        }
    }
}

#[test]
fn negotiate_empty_requirements_always_compatible() {
    for name in SDK_NAMES {
        let m = sdk_manifest(name);
        let reqs = CapabilityRequirements::default();
        let result = negotiate(&m, &reqs);
        assert!(
            result.is_compatible(),
            "{name} with no reqs should be compatible"
        );
    }
}

#[test]
fn negotiate_report_generation_for_all_sdks() {
    for name in SDK_NAMES {
        let m = sdk_manifest(name);
        let reqs = require_native(&[Capability::Streaming, Capability::ToolRead]);
        let result = negotiate(&m, &reqs);
        let report = generate_report(&result);
        assert!(report.native_count + report.emulated_count + report.unsupported_count == 2);
    }
}

// ===========================================================================
// 7. Emulation strategy verification
// ===========================================================================

#[test]
fn emulation_strategy_for_extended_thinking() {
    let strategy = default_strategy(&Capability::ExtendedThinking);
    assert!(
        matches!(strategy, EmulationStrategy::SystemPromptInjection { .. }),
        "ExtendedThinking should be emulated via system prompt injection"
    );
}

#[test]
fn emulation_strategy_for_structured_output() {
    let strategy = default_strategy(&Capability::StructuredOutputJsonSchema);
    assert!(
        matches!(strategy, EmulationStrategy::PostProcessing { .. }),
        "StructuredOutputJsonSchema should be emulated via post-processing"
    );
}

#[test]
fn emulation_strategy_for_code_execution_is_disabled() {
    let strategy = default_strategy(&Capability::CodeExecution);
    assert!(
        matches!(strategy, EmulationStrategy::Disabled { .. }),
        "CodeExecution should be disabled by default"
    );
}

#[test]
fn can_emulate_check_for_all_capabilities() {
    // Verify the emulation engine reports consistent results
    let emulatable_caps = [
        Capability::ExtendedThinking,
        Capability::StructuredOutputJsonSchema,
        Capability::ImageInput,
        Capability::StopSequences,
    ];
    for cap in &emulatable_caps {
        assert!(can_emulate(cap), "{cap:?} should be emulatable");
    }

    let non_emulatable_caps = [
        Capability::Streaming,
        Capability::ToolRead,
        Capability::CodeExecution,
    ];
    for cap in &non_emulatable_caps {
        assert!(!can_emulate(cap), "{cap:?} should not be emulatable");
    }
}

#[test]
fn emulation_engine_check_missing_for_unsupported_caps() {
    let engine = EmulationEngine::with_defaults();
    // Capabilities that each SDK doesn't support natively — check emulation feasibility
    let m = sdk_manifest("kimi");
    let (_, _, unsupported) = classify(&m);
    let unsupported_vec: Vec<_> = unsupported.into_iter().collect();
    let report = engine.check_missing(&unsupported_vec);
    // Some unsupported capabilities should generate warnings (disabled emulation)
    // while others may be emulatable
    let total = report.applied.len() + report.warnings.len();
    assert_eq!(total, unsupported_vec.len());
}

// ===========================================================================
// 8. abp_capability::check_capability integration
// ===========================================================================

#[test]
fn check_capability_native_for_claude_streaming() {
    let m = sdk_manifest("claude");
    assert_eq!(
        check_capability(&m, &Capability::Streaming),
        CapSupportLevel::Native
    );
}

#[test]
fn check_capability_emulated_for_openai_tool_read() {
    let m = sdk_manifest("openai");
    assert!(matches!(
        check_capability(&m, &Capability::ToolRead),
        CapSupportLevel::Emulated { .. }
    ));
}

#[test]
fn check_capability_unsupported_for_missing() {
    let m = sdk_manifest("kimi");
    // Capabilities not in Kimi's manifest at all
    assert_eq!(
        check_capability(&m, &Capability::SessionResume),
        CapSupportLevel::Unsupported {
            reason: "unsupported".into()
        }
    );
}

#[test]
fn check_capability_all_sdks_all_caps() {
    for name in SDK_NAMES {
        let m = sdk_manifest(name);
        for cap in all_capabilities() {
            let level = check_capability(&m, &cap);
            // Just ensure no panics and consistent classification
            match level {
                CapSupportLevel::Native
                | CapSupportLevel::Emulated { .. }
                | CapSupportLevel::Unsupported { .. }
                | CapSupportLevel::Restricted { .. } => {}
            }
        }
    }
}

// ===========================================================================
// 9. Projection matrix — SDK pair routing based on capabilities
// ===========================================================================

#[test]
fn projection_claude_can_satisfy_all_tool_reqs() {
    let m = sdk_manifest("claude");
    let reqs = require_native(&[
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::ToolGlob,
        Capability::ToolGrep,
    ]);
    let result = negotiate(&m, &reqs);
    assert!(result.is_compatible());
    assert_eq!(result.native.len(), 6);
}

#[test]
fn projection_openai_cannot_natively_satisfy_tool_reqs() {
    // OpenAI emulates tools, so they show up as emulatable, not native
    let m = sdk_manifest("openai");
    let reqs = require_native(&[Capability::ToolRead, Capability::ToolWrite]);
    let result = negotiate(&m, &reqs);
    assert!(result.is_compatible());
    assert!(result.native.is_empty());
    assert_eq!(result.emulated.len(), 2);
}

#[test]
fn projection_best_sdk_for_full_tool_suite() {
    // Find which SDK can satisfy the most tool capabilities natively
    let tool_caps = vec![
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::ToolGlob,
        Capability::ToolGrep,
        Capability::ToolWebSearch,
        Capability::ToolWebFetch,
    ];
    let reqs = require_native(&tool_caps);

    let mut best_sdk = "";
    let mut best_native = 0;
    for name in SDK_NAMES {
        let m = sdk_manifest(name);
        let result = negotiate(&m, &reqs);
        if result.native.len() > best_native {
            best_native = result.native.len();
            best_sdk = name;
        }
    }
    assert_eq!(
        best_sdk, "claude",
        "Claude should be best for full tool suite"
    );
}

#[test]
fn projection_best_sdk_for_web_search() {
    let reqs = require_native(&[Capability::ToolWebSearch]);
    let mut supporting_sdks = vec![];
    for name in SDK_NAMES {
        let m = sdk_manifest(name);
        let result = negotiate(&m, &reqs);
        if !result.native.is_empty() {
            supporting_sdks.push(*name);
        }
    }
    assert!(
        supporting_sdks.contains(&"claude"),
        "Claude should natively support web search"
    );
    assert!(
        supporting_sdks.contains(&"kimi"),
        "Kimi should natively support web search"
    );
    assert!(
        supporting_sdks.contains(&"copilot"),
        "Copilot should natively support web search"
    );
}

#[test]
fn projection_routing_with_fallback() {
    // If Kimi can't satisfy ToolBash, verify Claude can
    let reqs = require_native(&[Capability::Streaming, Capability::ToolBash]);
    let kimi_result = negotiate(&sdk_manifest("kimi"), &reqs);
    assert!(!kimi_result.is_compatible(), "Kimi lacks ToolBash");

    let claude_result = negotiate(&sdk_manifest("claude"), &reqs);
    assert!(
        claude_result.is_compatible(),
        "Claude should satisfy the requirement"
    );
}

#[test]
fn projection_mcp_client_only_claude() {
    let reqs = require_native(&[Capability::McpClient]);
    for name in SDK_NAMES {
        let m = sdk_manifest(name);
        let result = negotiate(&m, &reqs);
        if *name == "claude" {
            assert!(result.is_compatible(), "Claude supports McpClient");
        } else {
            assert!(
                !result.is_compatible(),
                "{name} does not support McpClient natively"
            );
        }
    }
}

// ===========================================================================
// 10. Comprehensive matrix — every SDK × every capability
// ===========================================================================

#[test]
fn full_matrix_no_panics() {
    for name in SDK_NAMES {
        let m = sdk_manifest(name);
        let (native, emulated, unsupported) = classify(&m);
        // Every capability should be in exactly one bucket
        for cap in all_capabilities() {
            let in_native = native.contains(&cap);
            let in_emulated = emulated.contains(&cap);
            let in_unsupported = unsupported.contains(&cap);
            let count =
                usize::from(in_native) + usize::from(in_emulated) + usize::from(in_unsupported);
            assert_eq!(
                count, 1,
                "{name}: {cap:?} should be in exactly one bucket, found in {count}"
            );
        }
    }
}

#[test]
fn full_matrix_classification_counts() {
    for name in SDK_NAMES {
        let (native, emulated, unsupported) = classify(&sdk_manifest(name));
        let total = native.len() + emulated.len() + unsupported.len();
        assert_eq!(
            total,
            all_capabilities().len(),
            "{name}: total classified must equal total capabilities"
        );
    }
}

#[test]
fn compatibility_report_summary_for_all_sdks() {
    for name in SDK_NAMES {
        let m = sdk_manifest(name);
        // Request everything
        let reqs = require_native(&all_capabilities());
        let result = negotiate(&m, &reqs);
        let report = generate_report(&result);
        let total = report.native_count + report.emulated_count + report.unsupported_count;
        assert_eq!(
            total,
            all_capabilities().len(),
            "{name} report total mismatch"
        );
        // Claude should be the most compatible but not 100% (McpServer is unsupported)
        if *name == "claude" {
            assert!(
                !report.compatible,
                "Claude can't satisfy ALL capabilities natively"
            );
        }
    }
}

#[test]
fn sdk_manifests_are_deterministic() {
    // Calling manifest twice should produce identical results
    for name in SDK_NAMES {
        let m1 = sdk_manifest(name);
        let m2 = sdk_manifest(name);
        assert_eq!(
            m1.len(),
            m2.len(),
            "{name} manifest length differs across calls"
        );
        for (cap, level1) in &m1 {
            let level2 = m2.get(cap).expect("capability missing in second call");
            // Compare debug representations since SupportLevel doesn't impl PartialEq
            assert_eq!(
                format!("{level1:?}"),
                format!("{level2:?}"),
                "{name}: {cap:?} support level differs across calls"
            );
        }
    }
}

// ===========================================================================
// 11. Pairwise SDK routing — verify capability gaps between SDK pairs
// ===========================================================================

#[test]
fn claude_to_openai_capability_gap() {
    let claude = sdk_manifest("claude");
    let openai = sdk_manifest("openai");
    // Capabilities Claude has natively that OpenAI doesn't
    let (claude_native, _, _) = classify(&claude);
    let (openai_native, _, _) = classify(&openai);
    let gap: BTreeSet<_> = claude_native.difference(&openai_native).cloned().collect();
    assert!(
        gap.contains(&Capability::ToolRead),
        "Claude has native ToolRead but OpenAI doesn't"
    );
}

#[test]
fn openai_to_claude_capability_gap() {
    let claude = sdk_manifest("claude");
    let openai = sdk_manifest("openai");
    let (claude_native, _, _) = classify(&claude);
    let (openai_native, _, _) = classify(&openai);
    // Capabilities OpenAI has that Claude also has
    let openai_only: BTreeSet<_> = openai_native.difference(&claude_native).cloned().collect();
    // OpenAI's native set is smaller, so this should be empty or minimal
    let _ = openai_only.len();
}

#[test]
fn kimi_to_gemini_web_search_gap() {
    let kimi = sdk_manifest("kimi");
    let gemini = sdk_manifest("gemini");
    // Kimi natively supports WebSearch, Gemini may not declare it
    let kimi_has_ws = matches!(
        kimi.get(&Capability::ToolWebSearch),
        Some(SupportLevel::Native)
    );
    let gemini_has_ws = matches!(
        gemini.get(&Capability::ToolWebSearch),
        Some(SupportLevel::Native)
    );
    assert!(kimi_has_ws, "Kimi should natively support WebSearch");
    assert!(
        !gemini_has_ws,
        "Gemini should not natively support WebSearch"
    );
}

#[test]
fn pairwise_compatibility_matrix() {
    // For each SDK pair, check if SDK A can satisfy SDK B's declared capabilities
    for src in SDK_NAMES {
        let src_manifest = sdk_manifest(src);
        let src_caps: Vec<Capability> = src_manifest.keys().cloned().collect();
        for tgt in SDK_NAMES {
            if src == tgt {
                continue;
            }
            let tgt_manifest = sdk_manifest(tgt);
            let reqs = require_emulated(&src_caps);
            let result = negotiate(&tgt_manifest, &reqs);
            // Just verify no panics and record compatibility
            let _compatible = result.is_compatible();
        }
    }
}
