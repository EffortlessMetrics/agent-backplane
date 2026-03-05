#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
#![allow(unused_imports)]

use abp_capability::negotiate::{apply_policy, pre_negotiate, NegotiationError, NegotiationPolicy};
use abp_capability::{
    check_capability, claude_35_sonnet_manifest, codex_manifest, copilot_manifest,
    default_emulation_strategy, gemini_15_pro_manifest, generate_report, kimi_manifest, negotiate,
    negotiate_capabilities, openai_gpt4o_manifest, CapabilityRegistry, CompatibilityReport,
    EmulationStrategy, NegotiationResult, SupportLevel,
};
use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    SupportLevel as CoreSupportLevel,
};
use std::collections::BTreeMap;

// ===========================================================================
// Helpers
// ===========================================================================

fn manifest_from(entries: &[(Capability, CoreSupportLevel)]) -> CapabilityManifest {
    entries.iter().cloned().collect()
}

fn require(caps: &[(Capability, MinSupport)]) -> CapabilityRequirements {
    CapabilityRequirements {
        required: caps
            .iter()
            .map(|(c, m)| CapabilityRequirement {
                capability: c.clone(),
                min_support: m.clone(),
            })
            .collect(),
    }
}

fn require_native(caps: &[Capability]) -> CapabilityRequirements {
    require(
        &caps
            .iter()
            .map(|c| (c.clone(), MinSupport::Native))
            .collect::<Vec<_>>(),
    )
}

fn require_emulated(caps: &[Capability]) -> CapabilityRequirements {
    require(
        &caps
            .iter()
            .map(|c| (c.clone(), MinSupport::Emulated))
            .collect::<Vec<_>>(),
    )
}

/// All known capability variants for exhaustive iteration.
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
        Capability::FunctionCalling,
        Capability::Vision,
        Capability::Audio,
        Capability::JsonMode,
        Capability::SystemMessage,
        Capability::Temperature,
        Capability::TopP,
        Capability::TopK,
        Capability::MaxTokens,
        Capability::FrequencyPenalty,
        Capability::PresencePenalty,
        Capability::CacheControl,
        Capability::BatchMode,
        Capability::Embeddings,
        Capability::ImageGeneration,
    ]
}

// ===========================================================================
// 1. Capability variant coverage
// ===========================================================================

#[test]
fn all_capability_variants_have_default_emulation_strategy() {
    for cap in all_capabilities() {
        let _ = default_emulation_strategy(&cap);
    }
}

#[test]
fn all_capability_variants_check_against_empty_manifest() {
    let empty: CapabilityManifest = BTreeMap::new();
    for cap in all_capabilities() {
        let level = check_capability(&empty, &cap);
        assert!(
            matches!(level, SupportLevel::Unsupported { .. }),
            "{cap:?} should be unsupported in empty manifest"
        );
    }
}

#[test]
fn all_capability_variants_native_in_full_manifest() {
    let mut manifest = CapabilityManifest::new();
    for cap in all_capabilities() {
        manifest.insert(cap, CoreSupportLevel::Native);
    }
    for cap in all_capabilities() {
        assert_eq!(
            check_capability(&manifest, &cap),
            SupportLevel::Native,
            "{cap:?} should be native"
        );
    }
}

#[test]
fn all_capability_variants_emulated_in_full_manifest() {
    let mut manifest = CapabilityManifest::new();
    for cap in all_capabilities() {
        manifest.insert(cap, CoreSupportLevel::Emulated);
    }
    for cap in all_capabilities() {
        assert!(
            matches!(
                check_capability(&manifest, &cap),
                SupportLevel::Emulated { .. }
            ),
            "{cap:?} should be emulated"
        );
    }
}

#[test]
fn all_capability_variants_restricted_in_full_manifest() {
    let mut manifest = CapabilityManifest::new();
    for cap in all_capabilities() {
        manifest.insert(
            cap,
            CoreSupportLevel::Restricted {
                reason: "test".into(),
            },
        );
    }
    for cap in all_capabilities() {
        assert!(
            matches!(
                check_capability(&manifest, &cap),
                SupportLevel::Restricted { .. }
            ),
            "{cap:?} should be restricted"
        );
    }
}

#[test]
fn all_capability_variants_explicit_unsupported_in_full_manifest() {
    let mut manifest = CapabilityManifest::new();
    for cap in all_capabilities() {
        manifest.insert(cap, CoreSupportLevel::Unsupported);
    }
    for cap in all_capabilities() {
        assert!(
            matches!(
                check_capability(&manifest, &cap),
                SupportLevel::Unsupported { .. }
            ),
            "{cap:?} should be unsupported"
        );
    }
}

// ===========================================================================
// 2. SupportLevel classification
// ===========================================================================

#[test]
fn support_level_native_display() {
    assert_eq!(SupportLevel::Native.to_string(), "native");
}

#[test]
fn support_level_emulated_display() {
    let s = SupportLevel::Emulated {
        method: "polyfill".into(),
    };
    assert_eq!(s.to_string(), "emulated (polyfill)");
}

#[test]
fn support_level_restricted_display() {
    let s = SupportLevel::Restricted {
        reason: "sandbox".into(),
    };
    assert_eq!(s.to_string(), "restricted (sandbox)");
}

#[test]
fn support_level_unsupported_display() {
    let s = SupportLevel::Unsupported {
        reason: "n/a".into(),
    };
    assert_eq!(s.to_string(), "unsupported (n/a)");
}

#[test]
fn support_level_native_serde_roundtrip() {
    let level = SupportLevel::Native;
    let json = serde_json::to_string(&level).unwrap();
    let back: SupportLevel = serde_json::from_str(&json).unwrap();
    assert_eq!(back, level);
}

#[test]
fn support_level_emulated_serde_roundtrip() {
    let level = SupportLevel::Emulated {
        method: "adapter".into(),
    };
    let json = serde_json::to_string(&level).unwrap();
    let back: SupportLevel = serde_json::from_str(&json).unwrap();
    assert_eq!(back, level);
}

#[test]
fn support_level_restricted_serde_roundtrip() {
    let level = SupportLevel::Restricted {
        reason: "sandboxed".into(),
    };
    let json = serde_json::to_string(&level).unwrap();
    let back: SupportLevel = serde_json::from_str(&json).unwrap();
    assert_eq!(back, level);
}

#[test]
fn support_level_unsupported_serde_roundtrip() {
    let level = SupportLevel::Unsupported {
        reason: "missing".into(),
    };
    let json = serde_json::to_string(&level).unwrap();
    let back: SupportLevel = serde_json::from_str(&json).unwrap();
    assert_eq!(back, level);
}

#[test]
fn support_level_json_contains_level_tag() {
    let json = serde_json::to_string(&SupportLevel::Native).unwrap();
    assert!(json.contains("\"level\""));
    assert!(json.contains("\"native\""));
}

// ===========================================================================
// 3. EmulationStrategy
// ===========================================================================

#[test]
fn emulation_strategy_client_side_display() {
    assert_eq!(
        EmulationStrategy::ClientSide.to_string(),
        "client-side emulation"
    );
}

#[test]
fn emulation_strategy_server_fallback_display() {
    assert_eq!(
        EmulationStrategy::ServerFallback.to_string(),
        "server fallback"
    );
}

#[test]
fn emulation_strategy_approximate_display() {
    assert_eq!(EmulationStrategy::Approximate.to_string(), "approximate");
}

#[test]
fn emulation_strategy_fidelity_loss_client_side() {
    assert!(!EmulationStrategy::ClientSide.has_fidelity_loss());
}

#[test]
fn emulation_strategy_fidelity_loss_server_fallback() {
    assert!(!EmulationStrategy::ServerFallback.has_fidelity_loss());
}

#[test]
fn emulation_strategy_fidelity_loss_approximate() {
    assert!(EmulationStrategy::Approximate.has_fidelity_loss());
}

#[test]
fn emulation_strategy_serde_roundtrip_all() {
    for strategy in [
        EmulationStrategy::ClientSide,
        EmulationStrategy::ServerFallback,
        EmulationStrategy::Approximate,
    ] {
        let json = serde_json::to_string(&strategy).unwrap();
        let back: EmulationStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(back, strategy);
    }
}

#[test]
fn emulation_strategy_json_values() {
    assert_eq!(
        serde_json::to_string(&EmulationStrategy::ClientSide).unwrap(),
        "\"client_side\""
    );
    assert_eq!(
        serde_json::to_string(&EmulationStrategy::ServerFallback).unwrap(),
        "\"server_fallback\""
    );
    assert_eq!(
        serde_json::to_string(&EmulationStrategy::Approximate).unwrap(),
        "\"approximate\""
    );
}

// ===========================================================================
// 4. default_emulation_strategy categorization
// ===========================================================================

#[test]
fn default_strategy_tool_capabilities_are_client_side() {
    let client_side_caps = [
        Capability::StructuredOutputJsonSchema,
        Capability::JsonMode,
        Capability::PdfInput,
        Capability::CodeExecution,
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
        Capability::Checkpointing,
    ];
    for cap in &client_side_caps {
        assert_eq!(
            default_emulation_strategy(cap),
            EmulationStrategy::ClientSide,
            "{cap:?} should have ClientSide strategy"
        );
    }
}

#[test]
fn default_strategy_server_fallback_capabilities() {
    let server_caps = [
        Capability::FunctionCalling,
        Capability::ToolUse,
        Capability::ExtendedThinking,
        Capability::BatchMode,
        Capability::SessionResume,
        Capability::SessionFork,
        Capability::McpClient,
        Capability::McpServer,
        Capability::SystemMessage,
    ];
    for cap in &server_caps {
        assert_eq!(
            default_emulation_strategy(cap),
            EmulationStrategy::ServerFallback,
            "{cap:?} should have ServerFallback strategy"
        );
    }
}

#[test]
fn default_strategy_approximate_capabilities() {
    let approx_caps = [
        Capability::Vision,
        Capability::ImageInput,
        Capability::Audio,
        Capability::ImageGeneration,
        Capability::Embeddings,
        Capability::CacheControl,
        Capability::Logprobs,
        Capability::SeedDeterminism,
        Capability::Streaming,
        Capability::StopSequences,
        Capability::Temperature,
        Capability::TopP,
        Capability::TopK,
        Capability::MaxTokens,
        Capability::FrequencyPenalty,
        Capability::PresencePenalty,
    ];
    for cap in &approx_caps {
        assert_eq!(
            default_emulation_strategy(cap),
            EmulationStrategy::Approximate,
            "{cap:?} should have Approximate strategy"
        );
    }
}

// ===========================================================================
// 5. NegotiationResult construction and methods
// ===========================================================================

#[test]
fn negotiation_result_from_simple_all_native() {
    let r = NegotiationResult::from_simple(
        vec![Capability::Streaming, Capability::ToolUse],
        vec![],
        vec![],
    );
    assert!(r.is_viable());
    assert!(r.is_compatible());
    assert_eq!(r.total(), 2);
    assert!(r.emulated.is_empty());
    assert!(r.unsupported.is_empty());
}

#[test]
fn negotiation_result_from_simple_emulated_uses_client_side() {
    let r = NegotiationResult::from_simple(vec![], vec![Capability::Vision], vec![]);
    assert_eq!(r.emulated.len(), 1);
    assert_eq!(r.emulated[0].1, EmulationStrategy::ClientSide);
}

#[test]
fn negotiation_result_from_simple_unsupported_has_reason() {
    let r = NegotiationResult::from_simple(vec![], vec![], vec![Capability::Audio]);
    assert_eq!(r.unsupported.len(), 1);
    assert_eq!(r.unsupported[0].1, "not available");
}

#[test]
fn negotiation_result_total_counts_all_buckets() {
    let r = NegotiationResult {
        native: vec![Capability::Streaming],
        emulated: vec![
            (Capability::ToolRead, EmulationStrategy::ClientSide),
            (Capability::ToolWrite, EmulationStrategy::ServerFallback),
        ],
        unsupported: vec![(Capability::Logprobs, "no".into())],
    };
    assert_eq!(r.total(), 4);
}

#[test]
fn negotiation_result_emulated_caps_extraction() {
    let r = NegotiationResult {
        native: vec![],
        emulated: vec![
            (Capability::ToolRead, EmulationStrategy::ClientSide),
            (Capability::Vision, EmulationStrategy::Approximate),
        ],
        unsupported: vec![],
    };
    assert_eq!(
        r.emulated_caps(),
        vec![Capability::ToolRead, Capability::Vision]
    );
}

#[test]
fn negotiation_result_unsupported_caps_extraction() {
    let r = NegotiationResult {
        native: vec![],
        emulated: vec![],
        unsupported: vec![
            (Capability::Audio, "no audio".into()),
            (Capability::Logprobs, "no logprobs".into()),
        ],
    };
    assert_eq!(
        r.unsupported_caps(),
        vec![Capability::Audio, Capability::Logprobs]
    );
}

#[test]
fn negotiation_result_warnings_only_approximate() {
    let r = NegotiationResult {
        native: vec![],
        emulated: vec![
            (Capability::ToolRead, EmulationStrategy::ClientSide),
            (
                Capability::FunctionCalling,
                EmulationStrategy::ServerFallback,
            ),
            (Capability::Vision, EmulationStrategy::Approximate),
        ],
        unsupported: vec![],
    };
    let w = r.warnings();
    assert_eq!(w.len(), 1);
    assert_eq!(w[0].0, Capability::Vision);
}

#[test]
fn negotiation_result_warnings_empty_when_no_approximate() {
    let r = NegotiationResult {
        native: vec![Capability::Streaming],
        emulated: vec![(Capability::ToolRead, EmulationStrategy::ClientSide)],
        unsupported: vec![],
    };
    assert!(r.warnings().is_empty());
}

#[test]
fn negotiation_result_is_viable_with_only_emulated() {
    let r = NegotiationResult {
        native: vec![],
        emulated: vec![(Capability::Streaming, EmulationStrategy::Approximate)],
        unsupported: vec![],
    };
    assert!(r.is_viable());
}

#[test]
fn negotiation_result_display_viable() {
    let r = NegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
    let s = r.to_string();
    assert!(s.contains("viable"));
    assert!(!s.contains("not viable"));
}

#[test]
fn negotiation_result_display_not_viable() {
    let r = NegotiationResult::from_simple(vec![], vec![], vec![Capability::Streaming]);
    let s = r.to_string();
    assert!(s.contains("not viable"));
}

#[test]
fn negotiation_result_display_counts() {
    let r = NegotiationResult {
        native: vec![Capability::Streaming, Capability::ToolUse],
        emulated: vec![(Capability::Vision, EmulationStrategy::Approximate)],
        unsupported: vec![(Capability::Audio, "no".into())],
    };
    let s = r.to_string();
    assert!(s.contains("2 native"));
    assert!(s.contains("1 emulated"));
    assert!(s.contains("1 unsupported"));
}

#[test]
fn negotiation_result_serde_roundtrip() {
    let r = NegotiationResult {
        native: vec![Capability::Streaming],
        emulated: vec![(Capability::ToolRead, EmulationStrategy::ClientSide)],
        unsupported: vec![(Capability::Logprobs, "no api".into())],
    };
    let json = serde_json::to_string(&r).unwrap();
    let back: NegotiationResult = serde_json::from_str(&json).unwrap();
    assert_eq!(back, r);
}

#[test]
fn negotiation_result_empty_serde() {
    let r = NegotiationResult {
        native: vec![],
        emulated: vec![],
        unsupported: vec![],
    };
    let json = serde_json::to_string(&r).unwrap();
    let back: NegotiationResult = serde_json::from_str(&json).unwrap();
    assert_eq!(back, r);
    assert!(back.is_viable());
}

// ===========================================================================
// 6. negotiate_capabilities (simple &[Capability] form)
// ===========================================================================

#[test]
fn negotiate_capabilities_all_native() {
    let m = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Native),
    ]);
    let r = negotiate_capabilities(&[Capability::Streaming, Capability::ToolUse], &m);
    assert_eq!(r.native.len(), 2);
    assert!(r.is_compatible());
}

#[test]
fn negotiate_capabilities_all_emulated() {
    let m = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Emulated),
        (Capability::Vision, CoreSupportLevel::Emulated),
    ]);
    let r = negotiate_capabilities(&[Capability::Streaming, Capability::Vision], &m);
    assert!(r.native.is_empty());
    assert_eq!(r.emulated.len(), 2);
    assert!(r.is_viable());
}

#[test]
fn negotiate_capabilities_all_unsupported() {
    let m: CapabilityManifest = BTreeMap::new();
    let r = negotiate_capabilities(&[Capability::Streaming, Capability::Vision], &m);
    assert_eq!(r.unsupported.len(), 2);
    assert!(!r.is_viable());
}

#[test]
fn negotiate_capabilities_mixed_native_emulated_unsupported() {
    let m = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::Vision, CoreSupportLevel::Emulated),
    ]);
    let r = negotiate_capabilities(
        &[Capability::Streaming, Capability::Vision, Capability::Audio],
        &m,
    );
    assert_eq!(r.native, vec![Capability::Streaming]);
    assert_eq!(r.emulated_caps(), vec![Capability::Vision]);
    assert_eq!(r.unsupported_caps(), vec![Capability::Audio]);
}

#[test]
fn negotiate_capabilities_empty_required() {
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = negotiate_capabilities(&[], &m);
    assert!(r.is_viable());
    assert_eq!(r.total(), 0);
}

#[test]
fn negotiate_capabilities_empty_manifest() {
    let r = negotiate_capabilities(&[Capability::Streaming], &BTreeMap::new());
    assert!(!r.is_viable());
    assert_eq!(r.unsupported.len(), 1);
}

#[test]
fn negotiate_capabilities_both_empty() {
    let r = negotiate_capabilities(&[], &BTreeMap::new());
    assert!(r.is_viable());
    assert_eq!(r.total(), 0);
}

#[test]
fn negotiate_capabilities_restricted_becomes_emulated() {
    let m = manifest_from(&[(
        Capability::ToolBash,
        CoreSupportLevel::Restricted {
            reason: "sandbox".into(),
        },
    )]);
    let r = negotiate_capabilities(&[Capability::ToolBash], &m);
    assert!(r.native.is_empty());
    assert_eq!(r.emulated.len(), 1);
    assert!(r.is_viable());
}

#[test]
fn negotiate_capabilities_explicit_unsupported() {
    let m = manifest_from(&[(Capability::Vision, CoreSupportLevel::Unsupported)]);
    let r = negotiate_capabilities(&[Capability::Vision], &m);
    assert_eq!(r.unsupported.len(), 1);
    assert!(!r.is_viable());
}

#[test]
fn negotiate_capabilities_preserves_order() {
    let m = manifest_from(&[
        (Capability::ToolWrite, CoreSupportLevel::Native),
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Native),
    ]);
    let r = negotiate_capabilities(
        &[
            Capability::ToolRead,
            Capability::Streaming,
            Capability::ToolWrite,
        ],
        &m,
    );
    assert_eq!(r.native[0], Capability::ToolRead);
    assert_eq!(r.native[1], Capability::Streaming);
    assert_eq!(r.native[2], Capability::ToolWrite);
}

#[test]
fn negotiate_capabilities_duplicate_entries() {
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = negotiate_capabilities(&[Capability::Streaming, Capability::Streaming], &m);
    assert_eq!(r.native.len(), 2);
}

#[test]
fn negotiate_capabilities_emulated_includes_strategy() {
    let m = manifest_from(&[(Capability::Vision, CoreSupportLevel::Emulated)]);
    let r = negotiate_capabilities(&[Capability::Vision], &m);
    assert_eq!(r.emulated[0].1, EmulationStrategy::Approximate);
}

#[test]
fn negotiate_capabilities_unsupported_includes_reason() {
    let m = manifest_from(&[(Capability::Logprobs, CoreSupportLevel::Unsupported)]);
    let r = negotiate_capabilities(&[Capability::Logprobs], &m);
    assert!(!r.unsupported[0].1.is_empty());
}

// ===========================================================================
// 7. negotiate (CapabilityRequirements form)
// ===========================================================================

#[test]
fn negotiate_req_all_native_satisfied() {
    let m = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Native),
    ]);
    let reqs = require_native(&[Capability::Streaming, Capability::ToolRead]);
    let r = negotiate(&m, &reqs);
    assert_eq!(r.native.len(), 2);
    assert!(r.is_compatible());
}

#[test]
fn negotiate_req_native_required_emulated_provided_fails() {
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
    let reqs = require_native(&[Capability::Streaming]);
    let r = negotiate(&m, &reqs);
    assert!(!r.is_compatible());
    assert_eq!(r.unsupported.len(), 1);
}

#[test]
fn negotiate_req_emulated_required_native_provided_succeeds() {
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let reqs = require_emulated(&[Capability::Streaming]);
    let r = negotiate(&m, &reqs);
    assert!(r.is_compatible());
    assert_eq!(r.native.len(), 1);
}

#[test]
fn negotiate_req_emulated_required_emulated_provided_succeeds() {
    let m = manifest_from(&[(Capability::Vision, CoreSupportLevel::Emulated)]);
    let reqs = require_emulated(&[Capability::Vision]);
    let r = negotiate(&m, &reqs);
    assert!(r.is_compatible());
    assert_eq!(r.emulated.len(), 1);
}

#[test]
fn negotiate_req_emulated_required_restricted_provided_succeeds() {
    let m = manifest_from(&[(
        Capability::ToolBash,
        CoreSupportLevel::Restricted {
            reason: "sandbox".into(),
        },
    )]);
    let reqs = require_emulated(&[Capability::ToolBash]);
    let r = negotiate(&m, &reqs);
    assert!(r.is_compatible());
    assert_eq!(r.emulated.len(), 1);
}

#[test]
fn negotiate_req_emulated_required_unsupported_fails() {
    let m = manifest_from(&[(Capability::Audio, CoreSupportLevel::Unsupported)]);
    let reqs = require_emulated(&[Capability::Audio]);
    let r = negotiate(&m, &reqs);
    assert!(!r.is_compatible());
    assert_eq!(r.unsupported.len(), 1);
}

#[test]
fn negotiate_req_empty_manifest() {
    let r = negotiate(&BTreeMap::new(), &require_native(&[Capability::Streaming]));
    assert!(!r.is_compatible());
}

#[test]
fn negotiate_req_empty_requirements() {
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = negotiate(&m, &CapabilityRequirements::default());
    assert!(r.is_compatible());
    assert_eq!(r.total(), 0);
}

#[test]
fn negotiate_req_mixed_min_support_levels() {
    let m = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::Vision, CoreSupportLevel::Emulated),
        (Capability::Audio, CoreSupportLevel::Emulated),
    ]);
    let reqs = require(&[
        (Capability::Streaming, MinSupport::Native),
        (Capability::Vision, MinSupport::Emulated),
        (Capability::Audio, MinSupport::Native),
    ]);
    let r = negotiate(&m, &reqs);
    assert_eq!(r.native.len(), 1);
    assert_eq!(r.emulated.len(), 1);
    assert_eq!(r.unsupported.len(), 1);
}

// ===========================================================================
// 8. check_capability
// ===========================================================================

#[test]
fn check_capability_native() {
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    assert_eq!(
        check_capability(&m, &Capability::Streaming),
        SupportLevel::Native
    );
}

#[test]
fn check_capability_emulated_returns_adapter_method() {
    let m = manifest_from(&[(Capability::Vision, CoreSupportLevel::Emulated)]);
    match check_capability(&m, &Capability::Vision) {
        SupportLevel::Emulated { method } => assert_eq!(method, "adapter"),
        other => panic!("expected Emulated, got {other:?}"),
    }
}

#[test]
fn check_capability_restricted_preserves_reason() {
    let m = manifest_from(&[(
        Capability::ToolBash,
        CoreSupportLevel::Restricted {
            reason: "policy-enforced".into(),
        },
    )]);
    match check_capability(&m, &Capability::ToolBash) {
        SupportLevel::Restricted { reason } => assert_eq!(reason, "policy-enforced"),
        other => panic!("expected Restricted, got {other:?}"),
    }
}

#[test]
fn check_capability_explicit_unsupported() {
    let m = manifest_from(&[(Capability::Audio, CoreSupportLevel::Unsupported)]);
    match check_capability(&m, &Capability::Audio) {
        SupportLevel::Unsupported { reason } => {
            assert!(reason.contains("explicitly marked unsupported"))
        }
        other => panic!("expected Unsupported, got {other:?}"),
    }
}

#[test]
fn check_capability_missing_from_manifest() {
    let m: CapabilityManifest = BTreeMap::new();
    match check_capability(&m, &Capability::Audio) {
        SupportLevel::Unsupported { reason } => {
            assert!(reason.contains("not declared in manifest"))
        }
        other => panic!("expected Unsupported, got {other:?}"),
    }
}

// ===========================================================================
// 9. generate_report
// ===========================================================================

#[test]
fn report_fully_compatible() {
    let r = NegotiationResult::from_simple(
        vec![Capability::Streaming, Capability::ToolUse],
        vec![Capability::Vision],
        vec![],
    );
    let report = generate_report(&r);
    assert!(report.compatible);
    assert_eq!(report.native_count, 2);
    assert_eq!(report.emulated_count, 1);
    assert_eq!(report.unsupported_count, 0);
    assert!(report.summary.contains("fully compatible"));
}

#[test]
fn report_incompatible() {
    let r = NegotiationResult::from_simple(vec![], vec![], vec![Capability::Streaming]);
    let report = generate_report(&r);
    assert!(!report.compatible);
    assert!(report.summary.contains("incompatible"));
}

#[test]
fn report_empty() {
    let r = NegotiationResult::from_simple(vec![], vec![], vec![]);
    let report = generate_report(&r);
    assert!(report.compatible);
    assert_eq!(report.native_count, 0);
    assert_eq!(report.emulated_count, 0);
    assert_eq!(report.unsupported_count, 0);
}

#[test]
fn report_details_length_matches_total() {
    let r = NegotiationResult {
        native: vec![Capability::Streaming],
        emulated: vec![(Capability::ToolRead, EmulationStrategy::ClientSide)],
        unsupported: vec![(Capability::Logprobs, "no".into())],
    };
    let report = generate_report(&r);
    assert_eq!(report.details.len(), 3);
}

#[test]
fn report_display_is_summary() {
    let r = NegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
    let report = generate_report(&r);
    assert_eq!(report.to_string(), report.summary);
}

#[test]
fn report_serde_roundtrip() {
    let r = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::Vision],
        vec![Capability::Audio],
    );
    let report = generate_report(&r);
    let json = serde_json::to_string(&report).unwrap();
    let back: CompatibilityReport = serde_json::from_str(&json).unwrap();
    assert_eq!(back, report);
}

// ===========================================================================
// 10. CapabilityRegistry
// ===========================================================================

#[test]
fn registry_new_is_empty() {
    let reg = CapabilityRegistry::new();
    assert!(reg.is_empty());
    assert_eq!(reg.len(), 0);
    assert!(reg.names().is_empty());
}

#[test]
fn registry_register_and_get() {
    let mut reg = CapabilityRegistry::new();
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    reg.register("my-backend", m);
    assert_eq!(reg.len(), 1);
    assert!(reg.contains("my-backend"));
    assert!(reg.get("my-backend").is_some());
}

#[test]
fn registry_get_missing_returns_none() {
    let reg = CapabilityRegistry::new();
    assert!(reg.get("nonexistent").is_none());
    assert!(!reg.contains("nonexistent"));
}

#[test]
fn registry_overwrite_replaces() {
    let mut reg = CapabilityRegistry::new();
    let m1 = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let m2 = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
    reg.register("x", m1);
    reg.register("x", m2);
    assert_eq!(reg.len(), 1);
    assert!(matches!(
        reg.get("x").unwrap().get(&Capability::Streaming),
        Some(CoreSupportLevel::Emulated)
    ));
}

#[test]
fn registry_unregister() {
    let mut reg = CapabilityRegistry::new();
    reg.register("a", BTreeMap::new());
    assert!(reg.unregister("a"));
    assert!(!reg.unregister("a"));
    assert!(reg.is_empty());
}

#[test]
fn registry_names_sorted() {
    let mut reg = CapabilityRegistry::new();
    reg.register("z-backend", BTreeMap::new());
    reg.register("a-backend", BTreeMap::new());
    reg.register("m-backend", BTreeMap::new());
    let names = reg.names();
    assert_eq!(names, vec!["a-backend", "m-backend", "z-backend"]);
}

#[test]
fn registry_negotiate_by_name_found() {
    let mut reg = CapabilityRegistry::new();
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    reg.register("b", m);
    let r = reg
        .negotiate_by_name("b", &[Capability::Streaming])
        .unwrap();
    assert!(r.is_compatible());
    assert_eq!(r.native, vec![Capability::Streaming]);
}

#[test]
fn registry_negotiate_by_name_missing() {
    let reg = CapabilityRegistry::new();
    assert!(reg
        .negotiate_by_name("missing", &[Capability::Streaming])
        .is_none());
}

#[test]
fn registry_with_defaults_six_backends() {
    let reg = CapabilityRegistry::with_defaults();
    assert_eq!(reg.len(), 6);
    assert!(reg.contains("openai/gpt-4o"));
    assert!(reg.contains("anthropic/claude-3.5-sonnet"));
    assert!(reg.contains("google/gemini-1.5-pro"));
    assert!(reg.contains("moonshot/kimi"));
    assert!(reg.contains("openai/codex"));
    assert!(reg.contains("github/copilot"));
}

#[test]
fn registry_query_capability_streaming_all_native() {
    let reg = CapabilityRegistry::with_defaults();
    let results = reg.query_capability(&Capability::Streaming);
    assert_eq!(results.len(), 6);
    for (name, level) in &results {
        assert!(
            matches!(level, SupportLevel::Native),
            "{name} should have native streaming"
        );
    }
}

#[test]
fn registry_query_capability_extended_thinking() {
    let reg = CapabilityRegistry::with_defaults();
    let results = reg.query_capability(&Capability::ExtendedThinking);
    let native_count = results
        .iter()
        .filter(|(_, l)| matches!(l, SupportLevel::Native))
        .count();
    assert_eq!(
        native_count, 1,
        "only Claude supports ExtendedThinking natively"
    );
}

#[test]
fn registry_compare_same_backend_is_viable() {
    let reg = CapabilityRegistry::with_defaults();
    let r = reg.compare("openai/gpt-4o", "openai/gpt-4o").unwrap();
    assert!(r.is_viable());
}

#[test]
fn registry_compare_claude_to_openai() {
    let reg = CapabilityRegistry::with_defaults();
    let r = reg
        .compare("anthropic/claude-3.5-sonnet", "openai/gpt-4o")
        .unwrap();
    assert!(
        r.unsupported_caps().contains(&Capability::ExtendedThinking),
        "OpenAI doesn't support ExtendedThinking"
    );
}

#[test]
fn registry_compare_missing_source() {
    let reg = CapabilityRegistry::with_defaults();
    assert!(reg.compare("nonexistent", "openai/gpt-4o").is_none());
}

#[test]
fn registry_compare_missing_target() {
    let reg = CapabilityRegistry::with_defaults();
    assert!(reg.compare("openai/gpt-4o", "nonexistent").is_none());
}

#[test]
fn registry_compare_both_missing() {
    let reg = CapabilityRegistry::with_defaults();
    assert!(reg.compare("a", "b").is_none());
}

// ===========================================================================
// 11. NegotiationPolicy & pre_negotiate & apply_policy
// ===========================================================================

#[test]
fn policy_default_is_strict() {
    assert_eq!(NegotiationPolicy::default(), NegotiationPolicy::Strict);
}

#[test]
fn policy_display_strict() {
    assert_eq!(NegotiationPolicy::Strict.to_string(), "strict");
}

#[test]
fn policy_display_best_effort() {
    assert_eq!(NegotiationPolicy::BestEffort.to_string(), "best-effort");
}

#[test]
fn policy_display_permissive() {
    assert_eq!(NegotiationPolicy::Permissive.to_string(), "permissive");
}

#[test]
fn policy_serde_roundtrip_all() {
    for policy in [
        NegotiationPolicy::Strict,
        NegotiationPolicy::BestEffort,
        NegotiationPolicy::Permissive,
    ] {
        let json = serde_json::to_string(&policy).unwrap();
        let back: NegotiationPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(back, policy);
    }
}

#[test]
fn policy_json_values() {
    assert_eq!(
        serde_json::to_string(&NegotiationPolicy::Strict).unwrap(),
        "\"strict\""
    );
    assert_eq!(
        serde_json::to_string(&NegotiationPolicy::BestEffort).unwrap(),
        "\"best_effort\""
    );
    assert_eq!(
        serde_json::to_string(&NegotiationPolicy::Permissive).unwrap(),
        "\"permissive\""
    );
}

#[test]
fn pre_negotiate_all_native() {
    let m = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Native),
    ]);
    let r = pre_negotiate(&[Capability::Streaming, Capability::ToolUse], &m);
    assert_eq!(r.native.len(), 2);
    assert!(r.is_viable());
}

#[test]
fn pre_negotiate_all_unsupported() {
    let r = pre_negotiate(
        &[Capability::Streaming, Capability::ToolUse],
        &BTreeMap::new(),
    );
    assert_eq!(r.unsupported.len(), 2);
    assert!(!r.is_viable());
}

#[test]
fn pre_negotiate_mixed() {
    let m = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Emulated),
    ]);
    let r = pre_negotiate(
        &[
            Capability::Streaming,
            Capability::ToolUse,
            Capability::Vision,
        ],
        &m,
    );
    assert_eq!(r.native.len(), 1);
    assert_eq!(r.emulated.len(), 1);
    assert_eq!(r.unsupported.len(), 1);
}

#[test]
fn pre_negotiate_empty() {
    let r = pre_negotiate(&[], &BTreeMap::new());
    assert!(r.is_viable());
    assert_eq!(r.total(), 0);
}

#[test]
fn apply_policy_strict_passes_native() {
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = pre_negotiate(&[Capability::Streaming], &m);
    assert!(apply_policy(&r, NegotiationPolicy::Strict).is_ok());
}

#[test]
fn apply_policy_strict_passes_emulated() {
    let m = manifest_from(&[(Capability::ToolUse, CoreSupportLevel::Emulated)]);
    let r = pre_negotiate(&[Capability::ToolUse], &m);
    assert!(apply_policy(&r, NegotiationPolicy::Strict).is_ok());
}

#[test]
fn apply_policy_strict_fails_unsupported() {
    let r = pre_negotiate(&[Capability::Streaming], &BTreeMap::new());
    let err = apply_policy(&r, NegotiationPolicy::Strict).unwrap_err();
    assert_eq!(err.policy, NegotiationPolicy::Strict);
    assert_eq!(err.unsupported.len(), 1);
}

#[test]
fn apply_policy_best_effort_passes_all_native() {
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = pre_negotiate(&[Capability::Streaming], &m);
    assert!(apply_policy(&r, NegotiationPolicy::BestEffort).is_ok());
}

#[test]
fn apply_policy_best_effort_fails_unsupported() {
    let r = pre_negotiate(&[Capability::Vision], &BTreeMap::new());
    let err = apply_policy(&r, NegotiationPolicy::BestEffort).unwrap_err();
    assert_eq!(err.policy, NegotiationPolicy::BestEffort);
}

#[test]
fn apply_policy_permissive_always_ok() {
    let r = pre_negotiate(
        &[Capability::Streaming, Capability::Vision, Capability::Audio],
        &BTreeMap::new(),
    );
    assert!(!r.is_viable());
    assert!(apply_policy(&r, NegotiationPolicy::Permissive).is_ok());
}

#[test]
fn apply_policy_permissive_with_native() {
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = pre_negotiate(&[Capability::Streaming], &m);
    assert!(apply_policy(&r, NegotiationPolicy::Permissive).is_ok());
}

// ===========================================================================
// 12. NegotiationError
// ===========================================================================

#[test]
fn negotiation_error_display_single() {
    let err = NegotiationError {
        policy: NegotiationPolicy::Strict,
        unsupported: vec![(Capability::Vision, "not available".into())],
        warnings: vec![],
    };
    let msg = err.to_string();
    assert!(msg.contains("strict"));
    assert!(msg.contains("1 unsupported"));
    assert!(msg.contains("Vision"));
}

#[test]
fn negotiation_error_display_multiple() {
    let err = NegotiationError {
        policy: NegotiationPolicy::BestEffort,
        unsupported: vec![
            (Capability::Vision, "no".into()),
            (Capability::Audio, "no".into()),
            (Capability::Logprobs, "no".into()),
        ],
        warnings: vec![],
    };
    let msg = err.to_string();
    assert!(msg.contains("3 unsupported"));
}

#[test]
fn negotiation_error_is_std_error() {
    let err = NegotiationError {
        policy: NegotiationPolicy::Strict,
        unsupported: vec![(Capability::Streaming, "missing".into())],
        warnings: vec![],
    };
    let _: &dyn std::error::Error = &err;
}

#[test]
fn negotiation_error_includes_warnings() {
    let m = manifest_from(&[(Capability::Vision, CoreSupportLevel::Emulated)]);
    let r = pre_negotiate(&[Capability::Vision, Capability::Audio], &m);
    let err = apply_policy(&r, NegotiationPolicy::Strict).unwrap_err();
    assert_eq!(err.unsupported.len(), 1);
}

// ===========================================================================
// 13. Pre-populated manifest checks
// ===========================================================================

#[test]
fn openai_manifest_streaming_native() {
    let m = openai_gpt4o_manifest();
    assert_eq!(
        check_capability(&m, &Capability::Streaming),
        SupportLevel::Native
    );
}

#[test]
fn openai_manifest_tool_use_native() {
    let m = openai_gpt4o_manifest();
    assert_eq!(
        check_capability(&m, &Capability::ToolUse),
        SupportLevel::Native
    );
}

#[test]
fn openai_manifest_extended_thinking_unsupported() {
    let m = openai_gpt4o_manifest();
    assert!(matches!(
        check_capability(&m, &Capability::ExtendedThinking),
        SupportLevel::Unsupported { .. }
    ));
}

#[test]
fn claude_manifest_extended_thinking_native() {
    let m = claude_35_sonnet_manifest();
    assert_eq!(
        check_capability(&m, &Capability::ExtendedThinking),
        SupportLevel::Native
    );
}

#[test]
fn claude_manifest_audio_unsupported() {
    let m = claude_35_sonnet_manifest();
    assert!(matches!(
        check_capability(&m, &Capability::Audio),
        SupportLevel::Unsupported { .. }
    ));
}

#[test]
fn gemini_manifest_code_execution_native() {
    let m = gemini_15_pro_manifest();
    assert_eq!(
        check_capability(&m, &Capability::CodeExecution),
        SupportLevel::Native
    );
}

#[test]
fn gemini_manifest_batch_mode_unsupported() {
    let m = gemini_15_pro_manifest();
    assert!(matches!(
        check_capability(&m, &Capability::BatchMode),
        SupportLevel::Unsupported { .. }
    ));
}

#[test]
fn kimi_manifest_vision_native() {
    let m = kimi_manifest();
    assert_eq!(
        check_capability(&m, &Capability::Vision),
        SupportLevel::Native
    );
}

#[test]
fn kimi_manifest_code_execution_unsupported() {
    let m = kimi_manifest();
    assert!(matches!(
        check_capability(&m, &Capability::CodeExecution),
        SupportLevel::Unsupported { .. }
    ));
}

#[test]
fn codex_manifest_tool_bash_native() {
    let m = codex_manifest();
    assert_eq!(
        check_capability(&m, &Capability::ToolBash),
        SupportLevel::Native
    );
}

#[test]
fn codex_manifest_vision_emulated() {
    let m = codex_manifest();
    assert!(matches!(
        check_capability(&m, &Capability::Vision),
        SupportLevel::Emulated { .. }
    ));
}

#[test]
fn copilot_manifest_tool_web_search_native() {
    let m = copilot_manifest();
    assert_eq!(
        check_capability(&m, &Capability::ToolWebSearch),
        SupportLevel::Native
    );
}

#[test]
fn copilot_manifest_audio_unsupported() {
    let m = copilot_manifest();
    assert!(matches!(
        check_capability(&m, &Capability::Audio),
        SupportLevel::Unsupported { .. }
    ));
}

// ===========================================================================
// 14. Cross-model negotiations
// ===========================================================================

#[test]
fn cross_model_streaming_all_native() {
    let required = &[Capability::Streaming];
    for manifest_fn in [
        openai_gpt4o_manifest,
        claude_35_sonnet_manifest,
        gemini_15_pro_manifest,
        kimi_manifest,
        codex_manifest,
        copilot_manifest,
    ] {
        let r = negotiate_capabilities(required, &manifest_fn());
        assert!(r.is_compatible());
        assert_eq!(r.native.len(), 1);
    }
}

#[test]
fn cross_model_extended_thinking_only_claude() {
    let required = &[Capability::ExtendedThinking];
    let openai = negotiate_capabilities(required, &openai_gpt4o_manifest());
    let claude = negotiate_capabilities(required, &claude_35_sonnet_manifest());
    let gemini = negotiate_capabilities(required, &gemini_15_pro_manifest());
    assert!(!openai.is_compatible());
    assert!(claude.is_compatible());
    assert!(!gemini.is_compatible());
}

#[test]
fn cross_model_audio_openai_and_gemini() {
    let required = &[Capability::Audio];
    let openai = negotiate_capabilities(required, &openai_gpt4o_manifest());
    let claude = negotiate_capabilities(required, &claude_35_sonnet_manifest());
    let gemini = negotiate_capabilities(required, &gemini_15_pro_manifest());
    assert!(openai.is_compatible());
    assert!(!claude.is_compatible());
    assert!(gemini.is_compatible());
}

#[test]
fn cross_model_codex_vs_copilot_tool_suite() {
    let tools = &[
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::ToolGlob,
        Capability::ToolGrep,
    ];
    let codex_r = negotiate_capabilities(tools, &codex_manifest());
    let copilot_r = negotiate_capabilities(tools, &copilot_manifest());
    assert!(codex_r.is_compatible());
    assert!(copilot_r.is_compatible());
}

#[test]
fn cross_model_copilot_exclusive_web_tools() {
    let required = &[
        Capability::ToolWebSearch,
        Capability::ToolWebFetch,
        Capability::ToolAskUser,
    ];
    let copilot_r = negotiate_capabilities(required, &copilot_manifest());
    assert!(copilot_r.is_compatible());
    assert_eq!(copilot_r.native.len(), 3);
}

// ===========================================================================
// 15. Multiple concurrent negotiations
// ===========================================================================

#[test]
fn multiple_negotiations_independent() {
    let m1 = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let m2 = manifest_from(&[(Capability::Vision, CoreSupportLevel::Native)]);
    let r1 = negotiate_capabilities(&[Capability::Streaming], &m1);
    let r2 = negotiate_capabilities(&[Capability::Vision], &m2);
    assert!(r1.is_compatible());
    assert!(r2.is_compatible());
    assert_eq!(r1.native, vec![Capability::Streaming]);
    assert_eq!(r2.native, vec![Capability::Vision]);
}

#[test]
fn registry_multiple_negotiation_by_name() {
    let reg = CapabilityRegistry::with_defaults();
    let required = &[Capability::Streaming, Capability::ToolUse];
    let openai_r = reg.negotiate_by_name("openai/gpt-4o", required).unwrap();
    let claude_r = reg
        .negotiate_by_name("anthropic/claude-3.5-sonnet", required)
        .unwrap();
    let gemini_r = reg
        .negotiate_by_name("google/gemini-1.5-pro", required)
        .unwrap();
    assert!(openai_r.is_compatible());
    assert!(claude_r.is_compatible());
    assert!(gemini_r.is_compatible());
}

#[test]
fn negotiate_same_manifest_different_requirements() {
    let m = openai_gpt4o_manifest();
    let r1 = negotiate_capabilities(&[Capability::Streaming], &m);
    let r2 = negotiate_capabilities(&[Capability::ExtendedThinking], &m);
    let r3 = negotiate_capabilities(&[Capability::Streaming, Capability::ExtendedThinking], &m);
    assert!(r1.is_compatible());
    assert!(!r2.is_compatible());
    assert!(!r3.is_compatible());
}

// ===========================================================================
// 16. Capability version compatibility (CONTRACT_VERSION awareness)
// ===========================================================================

#[test]
fn manifest_is_btreemap_for_deterministic_order() {
    let m = openai_gpt4o_manifest();
    let json1 = serde_json::to_string(&m).unwrap();
    let json2 = serde_json::to_string(&m).unwrap();
    assert_eq!(json1, json2, "BTreeMap ensures deterministic serialization");
}

#[test]
fn manifest_serde_roundtrip() {
    let m = openai_gpt4o_manifest();
    let json = serde_json::to_string(&m).unwrap();
    let back: CapabilityManifest = serde_json::from_str(&json).unwrap();
    let json2 = serde_json::to_string(&back).unwrap();
    assert_eq!(json, json2);
}

#[test]
fn all_default_manifests_serde_roundtrip() {
    for manifest_fn in [
        openai_gpt4o_manifest,
        claude_35_sonnet_manifest,
        gemini_15_pro_manifest,
        kimi_manifest,
        codex_manifest,
        copilot_manifest,
    ] {
        let m = manifest_fn();
        let json = serde_json::to_string(&m).unwrap();
        let back: CapabilityManifest = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&back).unwrap();
        assert_eq!(json, json2);
    }
}

// ===========================================================================
// 17. Capability serde
// ===========================================================================

#[test]
fn capability_serde_roundtrip_all() {
    for cap in all_capabilities() {
        let json = serde_json::to_string(&cap).unwrap();
        let back: Capability = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cap, "failed roundtrip for {cap:?}");
    }
}

#[test]
fn core_support_level_serde_roundtrip() {
    let levels = vec![
        CoreSupportLevel::Native,
        CoreSupportLevel::Emulated,
        CoreSupportLevel::Unsupported,
        CoreSupportLevel::Restricted {
            reason: "sandbox".into(),
        },
    ];
    for level in &levels {
        let json = serde_json::to_string(level).unwrap();
        let back: CoreSupportLevel = serde_json::from_str(&json).unwrap();
        assert_eq!(
            serde_json::to_string(&back).unwrap(),
            json,
            "failed for {level:?}"
        );
    }
}

#[test]
fn min_support_serde_roundtrip() {
    for ms in [MinSupport::Native, MinSupport::Emulated] {
        let json = serde_json::to_string(&ms).unwrap();
        let back: MinSupport = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&back).unwrap();
        assert_eq!(json, json2);
    }
}

#[test]
fn capability_requirements_serde_roundtrip() {
    let reqs = require(&[
        (Capability::Streaming, MinSupport::Native),
        (Capability::Vision, MinSupport::Emulated),
    ]);
    let json = serde_json::to_string(&reqs).unwrap();
    let back: CapabilityRequirements = serde_json::from_str(&json).unwrap();
    assert_eq!(back.required.len(), 2);
}

// ===========================================================================
// 18. Edge cases and boundary conditions
// ===========================================================================

#[test]
fn negotiate_very_large_requirement_set() {
    let m = openai_gpt4o_manifest();
    let caps = all_capabilities();
    let r = negotiate_capabilities(&caps, &m);
    assert_eq!(r.total(), caps.len());
    assert!(r.native.len() + r.emulated.len() + r.unsupported.len() == caps.len());
}

#[test]
fn negotiate_all_caps_against_full_native_manifest() {
    let mut m = CapabilityManifest::new();
    for cap in all_capabilities() {
        m.insert(cap, CoreSupportLevel::Native);
    }
    let r = negotiate_capabilities(&all_capabilities(), &m);
    assert!(r.is_viable());
    assert_eq!(r.native.len(), all_capabilities().len());
    assert!(r.emulated.is_empty());
    assert!(r.unsupported.is_empty());
}

#[test]
fn report_from_large_negotiation() {
    let m = openai_gpt4o_manifest();
    let r = negotiate_capabilities(&all_capabilities(), &m);
    let report = generate_report(&r);
    assert_eq!(
        report.native_count + report.emulated_count + report.unsupported_count,
        all_capabilities().len()
    );
}

#[test]
fn registry_add_custom_and_negotiate() {
    let mut reg = CapabilityRegistry::with_defaults();
    let mut custom = CapabilityManifest::new();
    custom.insert(Capability::Streaming, CoreSupportLevel::Native);
    custom.insert(Capability::ToolUse, CoreSupportLevel::Native);
    custom.insert(Capability::Vision, CoreSupportLevel::Emulated);
    reg.register("custom/my-model", custom);
    assert_eq!(reg.len(), 7);
    let r = reg
        .negotiate_by_name(
            "custom/my-model",
            &[Capability::Streaming, Capability::Vision],
        )
        .unwrap();
    assert!(r.is_compatible());
    assert_eq!(r.native.len(), 1);
    assert_eq!(r.emulated.len(), 1);
}

#[test]
fn registry_compare_custom_to_default() {
    let mut reg = CapabilityRegistry::with_defaults();
    let mut custom = CapabilityManifest::new();
    custom.insert(Capability::Streaming, CoreSupportLevel::Native);
    reg.register("custom/minimal", custom);
    let r = reg.compare("openai/gpt-4o", "custom/minimal").unwrap();
    assert!(!r.is_viable(), "minimal should miss many capabilities");
}

#[test]
fn negotiate_with_only_restricted_capabilities() {
    let m = manifest_from(&[
        (
            Capability::ToolBash,
            CoreSupportLevel::Restricted {
                reason: "sandbox only".into(),
            },
        ),
        (
            Capability::ToolWrite,
            CoreSupportLevel::Restricted {
                reason: "read-only".into(),
            },
        ),
    ]);
    let r = negotiate_capabilities(&[Capability::ToolBash, Capability::ToolWrite], &m);
    assert!(r.is_viable());
    assert_eq!(r.emulated.len(), 2);
    assert!(r.native.is_empty());
}

#[test]
fn policy_strict_with_restricted_passes() {
    let m = manifest_from(&[(
        Capability::ToolBash,
        CoreSupportLevel::Restricted {
            reason: "sandboxed".into(),
        },
    )]);
    let r = pre_negotiate(&[Capability::ToolBash], &m);
    assert!(apply_policy(&r, NegotiationPolicy::Strict).is_ok());
}

#[test]
fn policy_best_effort_with_emulated_passes() {
    let m = manifest_from(&[(Capability::Vision, CoreSupportLevel::Emulated)]);
    let r = pre_negotiate(&[Capability::Vision], &m);
    assert!(apply_policy(&r, NegotiationPolicy::BestEffort).is_ok());
}

#[test]
fn negotiation_error_clone() {
    let err = NegotiationError {
        policy: NegotiationPolicy::Strict,
        unsupported: vec![(Capability::Audio, "no".into())],
        warnings: vec![Capability::Vision],
    };
    let cloned = err.clone();
    assert_eq!(cloned, err);
}

#[test]
fn negotiation_result_clone() {
    let r = NegotiationResult {
        native: vec![Capability::Streaming],
        emulated: vec![(Capability::Vision, EmulationStrategy::Approximate)],
        unsupported: vec![(Capability::Audio, "no".into())],
    };
    let cloned = r.clone();
    assert_eq!(cloned, r);
}

#[test]
fn registry_clone() {
    let reg = CapabilityRegistry::with_defaults();
    let cloned = reg.clone();
    assert_eq!(cloned.len(), reg.len());
    assert_eq!(cloned.names(), reg.names());
}

#[test]
fn emulation_strategy_eq_and_hash() {
    use std::collections::HashSet;
    let mut set = HashSet::new();
    set.insert(EmulationStrategy::ClientSide);
    set.insert(EmulationStrategy::ServerFallback);
    set.insert(EmulationStrategy::Approximate);
    assert_eq!(set.len(), 3);
    set.insert(EmulationStrategy::ClientSide);
    assert_eq!(set.len(), 3);
}

#[test]
fn policy_eq_and_hash() {
    use std::collections::HashSet;
    let mut set = HashSet::new();
    set.insert(NegotiationPolicy::Strict);
    set.insert(NegotiationPolicy::BestEffort);
    set.insert(NegotiationPolicy::Permissive);
    assert_eq!(set.len(), 3);
}

#[test]
fn negotiation_result_empty_is_viable() {
    let r = NegotiationResult {
        native: vec![],
        emulated: vec![],
        unsupported: vec![],
    };
    assert!(r.is_viable());
    assert!(r.is_compatible());
    assert_eq!(r.total(), 0);
    assert!(r.warnings().is_empty());
    assert!(r.emulated_caps().is_empty());
    assert!(r.unsupported_caps().is_empty());
}

#[test]
fn compatibility_report_summary_format() {
    let r = NegotiationResult {
        native: vec![
            Capability::Streaming,
            Capability::ToolUse,
            Capability::Vision,
        ],
        emulated: vec![(Capability::ToolRead, EmulationStrategy::ClientSide)],
        unsupported: vec![],
    };
    let report = generate_report(&r);
    assert!(report.summary.contains("3 native"));
    assert!(report.summary.contains("1 emulated"));
    assert!(report.summary.contains("0 unsupported"));
    assert!(report.summary.contains("fully compatible"));
}

#[test]
fn all_default_manifests_negotiate_streaming_native() {
    let reg = CapabilityRegistry::with_defaults();
    for name in reg.names() {
        let r = reg
            .negotiate_by_name(name, &[Capability::Streaming])
            .unwrap();
        assert!(r.is_compatible(), "{name} should support streaming");
        assert_eq!(r.native.len(), 1, "{name} streaming should be native");
    }
}
