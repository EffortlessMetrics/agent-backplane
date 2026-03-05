#![allow(clippy::all)]
#![allow(unknown_lints)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(dead_code)]
#![allow(unused_must_use)]

use abp_capability::negotiate::{NegotiationError, NegotiationPolicy, apply_policy, pre_negotiate};
use abp_capability::{
    CapabilityRegistry, CompatibilityReport, EmulationStrategy, NegotiationResult, SupportLevel,
    check_capability, claude_35_sonnet_manifest, codex_manifest, copilot_manifest,
    default_emulation_strategy, gemini_15_pro_manifest, generate_report, kimi_manifest, negotiate,
    negotiate_capabilities, openai_gpt4o_manifest,
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

/// All Capability variants for exhaustive testing.
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
// EmulationStrategy tests
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
fn emulation_strategy_clone() {
    let s = EmulationStrategy::ClientSide;
    let cloned = s.clone();
    assert_eq!(s, cloned);
}

#[test]
fn emulation_strategy_debug() {
    let s = EmulationStrategy::Approximate;
    let dbg = format!("{:?}", s);
    assert!(dbg.contains("Approximate"));
}

#[test]
fn emulation_strategy_eq_same() {
    assert_eq!(EmulationStrategy::ClientSide, EmulationStrategy::ClientSide);
}

#[test]
fn emulation_strategy_ne_different() {
    assert_ne!(
        EmulationStrategy::ClientSide,
        EmulationStrategy::Approximate
    );
}

#[test]
fn emulation_strategy_serde_roundtrip_client_side() {
    let s = EmulationStrategy::ClientSide;
    let json = serde_json::to_string(&s).unwrap();
    let back: EmulationStrategy = serde_json::from_str(&json).unwrap();
    assert_eq!(back, s);
}

#[test]
fn emulation_strategy_serde_roundtrip_server_fallback() {
    let s = EmulationStrategy::ServerFallback;
    let json = serde_json::to_string(&s).unwrap();
    let back: EmulationStrategy = serde_json::from_str(&json).unwrap();
    assert_eq!(back, s);
}

#[test]
fn emulation_strategy_serde_roundtrip_approximate() {
    let s = EmulationStrategy::Approximate;
    let json = serde_json::to_string(&s).unwrap();
    let back: EmulationStrategy = serde_json::from_str(&json).unwrap();
    assert_eq!(back, s);
}

#[test]
fn emulation_strategy_serde_json_values() {
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

#[test]
fn emulation_strategy_deserialize_from_string() {
    let cs: EmulationStrategy = serde_json::from_str("\"client_side\"").unwrap();
    assert_eq!(cs, EmulationStrategy::ClientSide);
    let sf: EmulationStrategy = serde_json::from_str("\"server_fallback\"").unwrap();
    assert_eq!(sf, EmulationStrategy::ServerFallback);
    let ap: EmulationStrategy = serde_json::from_str("\"approximate\"").unwrap();
    assert_eq!(ap, EmulationStrategy::Approximate);
}

#[test]
fn emulation_strategy_hash_eq() {
    use std::collections::HashSet;
    let mut set = HashSet::new();
    set.insert(EmulationStrategy::ClientSide);
    set.insert(EmulationStrategy::ClientSide);
    assert_eq!(set.len(), 1);
    set.insert(EmulationStrategy::Approximate);
    assert_eq!(set.len(), 2);
}

// ===========================================================================
// SupportLevel tests
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
        reason: "N/A".into(),
    };
    assert_eq!(s.to_string(), "unsupported (N/A)");
}

#[test]
fn support_level_emulated_empty_method() {
    let s = SupportLevel::Emulated { method: "".into() };
    assert_eq!(s.to_string(), "emulated ()");
}

#[test]
fn support_level_unsupported_empty_reason() {
    let s = SupportLevel::Unsupported { reason: "".into() };
    assert_eq!(s.to_string(), "unsupported ()");
}

#[test]
fn support_level_clone() {
    let s = SupportLevel::Emulated {
        method: "adapter".into(),
    };
    let cloned = s.clone();
    assert_eq!(s, cloned);
}

#[test]
fn support_level_debug() {
    let dbg = format!("{:?}", SupportLevel::Native);
    assert!(dbg.contains("Native"));
}

#[test]
fn support_level_serde_roundtrip_native() {
    let s = SupportLevel::Native;
    let json = serde_json::to_string(&s).unwrap();
    let back: SupportLevel = serde_json::from_str(&json).unwrap();
    assert_eq!(back, s);
}

#[test]
fn support_level_serde_roundtrip_emulated() {
    let s = SupportLevel::Emulated {
        method: "polyfill".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let back: SupportLevel = serde_json::from_str(&json).unwrap();
    assert_eq!(back, s);
}

#[test]
fn support_level_serde_roundtrip_restricted() {
    let s = SupportLevel::Restricted {
        reason: "sandbox only".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let back: SupportLevel = serde_json::from_str(&json).unwrap();
    assert_eq!(back, s);
}

#[test]
fn support_level_serde_roundtrip_unsupported() {
    let s = SupportLevel::Unsupported {
        reason: "not available".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let back: SupportLevel = serde_json::from_str(&json).unwrap();
    assert_eq!(back, s);
}

#[test]
fn support_level_serde_tag_field() {
    let json = serde_json::to_string(&SupportLevel::Native).unwrap();
    assert!(json.contains("\"level\""));
    assert!(json.contains("\"native\""));
}

#[test]
fn support_level_ne_variants() {
    assert_ne!(
        SupportLevel::Native,
        SupportLevel::Unsupported { reason: "x".into() }
    );
}

#[test]
fn support_level_unicode_reason() {
    let s = SupportLevel::Unsupported {
        reason: "не поддерживается 🚫".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let back: SupportLevel = serde_json::from_str(&json).unwrap();
    assert_eq!(back, s);
}

#[test]
fn support_level_special_chars_method() {
    let s = SupportLevel::Emulated {
        method: "a\"b\\c\nnewline".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let back: SupportLevel = serde_json::from_str(&json).unwrap();
    assert_eq!(back, s);
}

// ===========================================================================
// NegotiationResult tests
// ===========================================================================

#[test]
fn negotiation_result_is_viable_empty() {
    let r = NegotiationResult {
        native: vec![],
        emulated: vec![],
        unsupported: vec![],
    };
    assert!(r.is_viable());
}

#[test]
fn negotiation_result_is_viable_with_native_only() {
    let r = NegotiationResult {
        native: vec![Capability::Streaming],
        emulated: vec![],
        unsupported: vec![],
    };
    assert!(r.is_viable());
}

#[test]
fn negotiation_result_is_viable_with_emulated_only() {
    let r = NegotiationResult {
        native: vec![],
        emulated: vec![(Capability::Vision, EmulationStrategy::Approximate)],
        unsupported: vec![],
    };
    assert!(r.is_viable());
}

#[test]
fn negotiation_result_not_viable_with_unsupported() {
    let r = NegotiationResult {
        native: vec![Capability::Streaming],
        emulated: vec![],
        unsupported: vec![(Capability::Audio, "missing".into())],
    };
    assert!(!r.is_viable());
}

#[test]
fn negotiation_result_is_compatible_alias() {
    let r = NegotiationResult {
        native: vec![],
        emulated: vec![],
        unsupported: vec![],
    };
    assert_eq!(r.is_viable(), r.is_compatible());

    let r2 = NegotiationResult {
        native: vec![],
        emulated: vec![],
        unsupported: vec![(Capability::Vision, "x".into())],
    };
    assert_eq!(r2.is_viable(), r2.is_compatible());
}

#[test]
fn negotiation_result_total_zero() {
    let r = NegotiationResult {
        native: vec![],
        emulated: vec![],
        unsupported: vec![],
    };
    assert_eq!(r.total(), 0);
}

#[test]
fn negotiation_result_total_mixed() {
    let r = NegotiationResult {
        native: vec![Capability::Streaming, Capability::ToolUse],
        emulated: vec![(Capability::Vision, EmulationStrategy::Approximate)],
        unsupported: vec![
            (Capability::Audio, "x".into()),
            (Capability::Logprobs, "y".into()),
        ],
    };
    assert_eq!(r.total(), 5);
}

#[test]
fn negotiation_result_warnings_no_approximate() {
    let r = NegotiationResult {
        native: vec![],
        emulated: vec![
            (Capability::ToolRead, EmulationStrategy::ClientSide),
            (
                Capability::FunctionCalling,
                EmulationStrategy::ServerFallback,
            ),
        ],
        unsupported: vec![],
    };
    assert!(r.warnings().is_empty());
}

#[test]
fn negotiation_result_warnings_all_approximate() {
    let r = NegotiationResult {
        native: vec![],
        emulated: vec![
            (Capability::Vision, EmulationStrategy::Approximate),
            (Capability::Audio, EmulationStrategy::Approximate),
        ],
        unsupported: vec![],
    };
    assert_eq!(r.warnings().len(), 2);
}

#[test]
fn negotiation_result_warnings_mixed_strategies() {
    let r = NegotiationResult {
        native: vec![],
        emulated: vec![
            (Capability::ToolRead, EmulationStrategy::ClientSide),
            (Capability::Vision, EmulationStrategy::Approximate),
            (
                Capability::FunctionCalling,
                EmulationStrategy::ServerFallback,
            ),
        ],
        unsupported: vec![],
    };
    let w = r.warnings();
    assert_eq!(w.len(), 1);
    assert_eq!(w[0].0, Capability::Vision);
}

#[test]
fn negotiation_result_emulated_caps_empty() {
    let r = NegotiationResult {
        native: vec![],
        emulated: vec![],
        unsupported: vec![],
    };
    assert!(r.emulated_caps().is_empty());
}

#[test]
fn negotiation_result_emulated_caps_extracts_names() {
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
fn negotiation_result_unsupported_caps_empty() {
    let r = NegotiationResult {
        native: vec![],
        emulated: vec![],
        unsupported: vec![],
    };
    assert!(r.unsupported_caps().is_empty());
}

#[test]
fn negotiation_result_unsupported_caps_extracts_names() {
    let r = NegotiationResult {
        native: vec![],
        emulated: vec![],
        unsupported: vec![
            (Capability::Logprobs, "no API".into()),
            (Capability::Audio, "not supported".into()),
        ],
    };
    assert_eq!(
        r.unsupported_caps(),
        vec![Capability::Logprobs, Capability::Audio]
    );
}

#[test]
fn negotiation_result_from_simple_all_empty() {
    let r = NegotiationResult::from_simple(vec![], vec![], vec![]);
    assert!(r.native.is_empty());
    assert!(r.emulated.is_empty());
    assert!(r.unsupported.is_empty());
    assert!(r.is_viable());
    assert_eq!(r.total(), 0);
}

#[test]
fn negotiation_result_from_simple_emulated_uses_client_side() {
    let r = NegotiationResult::from_simple(vec![], vec![Capability::ToolRead], vec![]);
    assert_eq!(r.emulated.len(), 1);
    assert_eq!(r.emulated[0].1, EmulationStrategy::ClientSide);
}

#[test]
fn negotiation_result_from_simple_unsupported_reason() {
    let r = NegotiationResult::from_simple(vec![], vec![], vec![Capability::Logprobs]);
    assert_eq!(r.unsupported.len(), 1);
    assert_eq!(r.unsupported[0].1, "not available");
}

#[test]
fn negotiation_result_from_simple_mixed() {
    let r = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead],
        vec![Capability::Logprobs],
    );
    assert_eq!(r.native, vec![Capability::Streaming]);
    assert_eq!(r.emulated_caps(), vec![Capability::ToolRead]);
    assert_eq!(r.unsupported_caps(), vec![Capability::Logprobs]);
    assert_eq!(r.total(), 3);
    assert!(!r.is_viable());
}

#[test]
fn negotiation_result_display_viable() {
    let r = NegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
    let s = r.to_string();
    assert!(s.contains("1 native"));
    assert!(s.contains("0 emulated"));
    assert!(s.contains("0 unsupported"));
    assert!(s.contains("viable"));
    assert!(!s.contains("not viable"));
}

#[test]
fn negotiation_result_display_not_viable() {
    let r = NegotiationResult::from_simple(vec![], vec![], vec![Capability::Audio]);
    let s = r.to_string();
    assert!(s.contains("not viable"));
}

#[test]
fn negotiation_result_display_counts_match() {
    let r = NegotiationResult {
        native: vec![Capability::Streaming, Capability::ToolUse],
        emulated: vec![(Capability::Vision, EmulationStrategy::Approximate)],
        unsupported: vec![(Capability::Audio, "x".into())],
    };
    let s = r.to_string();
    assert!(s.contains("2 native"));
    assert!(s.contains("1 emulated"));
    assert!(s.contains("1 unsupported"));
}

#[test]
fn negotiation_result_clone() {
    let r = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead],
        vec![Capability::Logprobs],
    );
    let cloned = r.clone();
    assert_eq!(r, cloned);
}

#[test]
fn negotiation_result_debug() {
    let r = NegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
    let dbg = format!("{:?}", r);
    assert!(dbg.contains("NegotiationResult"));
}

#[test]
fn negotiation_result_serde_roundtrip() {
    let r = NegotiationResult {
        native: vec![Capability::Streaming],
        emulated: vec![(Capability::ToolRead, EmulationStrategy::ClientSide)],
        unsupported: vec![(Capability::Logprobs, "no API".into())],
    };
    let json = serde_json::to_string(&r).unwrap();
    let back: NegotiationResult = serde_json::from_str(&json).unwrap();
    assert_eq!(back, r);
}

#[test]
fn negotiation_result_serde_roundtrip_empty() {
    let r = NegotiationResult {
        native: vec![],
        emulated: vec![],
        unsupported: vec![],
    };
    let json = serde_json::to_string(&r).unwrap();
    let back: NegotiationResult = serde_json::from_str(&json).unwrap();
    assert_eq!(back, r);
}

// ===========================================================================
// CompatibilityReport tests
// ===========================================================================

#[test]
fn compatibility_report_compatible() {
    let r = NegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
    let report = generate_report(&r);
    assert!(report.compatible);
    assert_eq!(report.native_count, 1);
    assert_eq!(report.emulated_count, 0);
    assert_eq!(report.unsupported_count, 0);
    assert!(report.summary.contains("fully compatible"));
}

#[test]
fn compatibility_report_incompatible() {
    let r = NegotiationResult::from_simple(vec![], vec![], vec![Capability::Audio]);
    let report = generate_report(&r);
    assert!(!report.compatible);
    assert!(report.summary.contains("incompatible"));
}

#[test]
fn compatibility_report_empty_result() {
    let r = NegotiationResult::from_simple(vec![], vec![], vec![]);
    let report = generate_report(&r);
    assert!(report.compatible);
    assert_eq!(report.native_count, 0);
    assert_eq!(report.emulated_count, 0);
    assert_eq!(report.unsupported_count, 0);
}

#[test]
fn compatibility_report_details_length() {
    let r = NegotiationResult {
        native: vec![Capability::Streaming],
        emulated: vec![(Capability::ToolRead, EmulationStrategy::ClientSide)],
        unsupported: vec![(Capability::Logprobs, "N/A".into())],
    };
    let report = generate_report(&r);
    assert_eq!(report.details.len(), 3);
}

#[test]
fn compatibility_report_details_native_entry() {
    let r = NegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
    let report = generate_report(&r);
    assert_eq!(report.details.len(), 1);
    assert!(matches!(report.details[0].1, SupportLevel::Native));
}

#[test]
fn compatibility_report_details_emulated_entry() {
    let r = NegotiationResult::from_simple(vec![], vec![Capability::ToolRead], vec![]);
    let report = generate_report(&r);
    assert_eq!(report.details.len(), 1);
    assert!(matches!(report.details[0].1, SupportLevel::Emulated { .. }));
}

#[test]
fn compatibility_report_details_unsupported_entry() {
    let r = NegotiationResult::from_simple(vec![], vec![], vec![Capability::Logprobs]);
    let report = generate_report(&r);
    assert_eq!(report.details.len(), 1);
    assert!(matches!(
        report.details[0].1,
        SupportLevel::Unsupported { .. }
    ));
}

#[test]
fn compatibility_report_display() {
    let r = NegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
    let report = generate_report(&r);
    let s = report.to_string();
    assert!(s.contains("fully compatible"));
}

#[test]
fn compatibility_report_serde_roundtrip() {
    let r = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead],
        vec![Capability::Logprobs],
    );
    let report = generate_report(&r);
    let json = serde_json::to_string(&report).unwrap();
    let back: CompatibilityReport = serde_json::from_str(&json).unwrap();
    assert_eq!(back, report);
}

#[test]
fn compatibility_report_clone() {
    let r = NegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
    let report = generate_report(&r);
    let cloned = report.clone();
    assert_eq!(report, cloned);
}

#[test]
fn compatibility_report_debug() {
    let r = NegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
    let report = generate_report(&r);
    let dbg = format!("{:?}", report);
    assert!(dbg.contains("CompatibilityReport"));
}

#[test]
fn compatibility_report_summary_counts() {
    let r = NegotiationResult {
        native: vec![Capability::Streaming, Capability::ToolUse],
        emulated: vec![(Capability::ToolBash, EmulationStrategy::ClientSide)],
        unsupported: vec![],
    };
    let report = generate_report(&r);
    assert!(report.summary.contains("2 native"));
    assert!(report.summary.contains("1 emulated"));
    assert!(report.summary.contains("0 unsupported"));
}

// ===========================================================================
// check_capability tests
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
fn check_capability_emulated() {
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
    let level = check_capability(&m, &Capability::Streaming);
    assert!(matches!(level, SupportLevel::Emulated { method } if method == "adapter"));
}

#[test]
fn check_capability_restricted() {
    let m = manifest_from(&[(
        Capability::ToolBash,
        CoreSupportLevel::Restricted {
            reason: "sandboxed".into(),
        },
    )]);
    let level = check_capability(&m, &Capability::ToolBash);
    assert!(matches!(level, SupportLevel::Restricted { reason } if reason == "sandboxed"));
}

#[test]
fn check_capability_unsupported_explicit() {
    let m = manifest_from(&[(Capability::Logprobs, CoreSupportLevel::Unsupported)]);
    let level = check_capability(&m, &Capability::Logprobs);
    assert!(matches!(level, SupportLevel::Unsupported { reason } if reason.contains("explicitly")));
}

#[test]
fn check_capability_missing_from_manifest() {
    let m: CapabilityManifest = BTreeMap::new();
    let level = check_capability(&m, &Capability::Streaming);
    assert!(
        matches!(level, SupportLevel::Unsupported { reason } if reason.contains("not declared"))
    );
}

#[test]
fn check_capability_empty_manifest_all_unsupported() {
    let m: CapabilityManifest = BTreeMap::new();
    for cap in all_capabilities() {
        assert!(matches!(
            check_capability(&m, &cap),
            SupportLevel::Unsupported { .. }
        ));
    }
}

// ===========================================================================
// negotiate_capabilities tests
// ===========================================================================

#[test]
fn negotiate_capabilities_all_native() {
    let m = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Native),
    ]);
    let r = negotiate_capabilities(&[Capability::Streaming, Capability::ToolUse], &m);
    assert_eq!(r.native.len(), 2);
    assert!(r.emulated.is_empty());
    assert!(r.unsupported.is_empty());
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
    assert!(r.unsupported.is_empty());
}

#[test]
fn negotiate_capabilities_all_unsupported() {
    let m: CapabilityManifest = BTreeMap::new();
    let r = negotiate_capabilities(&[Capability::Streaming, Capability::Vision], &m);
    assert!(r.native.is_empty());
    assert!(r.emulated.is_empty());
    assert_eq!(r.unsupported.len(), 2);
}

#[test]
fn negotiate_capabilities_mixed() {
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
    assert_eq!(r.total(), 0);
    assert!(r.is_compatible());
}

#[test]
fn negotiate_capabilities_empty_manifest() {
    let r = negotiate_capabilities(&[Capability::Streaming], &BTreeMap::new());
    assert_eq!(r.unsupported.len(), 1);
    assert!(!r.is_compatible());
}

#[test]
fn negotiate_capabilities_both_empty() {
    let r = negotiate_capabilities(&[], &BTreeMap::new());
    assert_eq!(r.total(), 0);
    assert!(r.is_compatible());
}

#[test]
fn negotiate_capabilities_restricted_treated_as_emulated() {
    let m = manifest_from(&[(
        Capability::ToolBash,
        CoreSupportLevel::Restricted {
            reason: "sandbox".into(),
        },
    )]);
    let r = negotiate_capabilities(&[Capability::ToolBash], &m);
    assert!(r.native.is_empty());
    assert_eq!(r.emulated.len(), 1);
    assert!(r.unsupported.is_empty());
}

#[test]
fn negotiate_capabilities_explicit_unsupported() {
    let m = manifest_from(&[(Capability::Vision, CoreSupportLevel::Unsupported)]);
    let r = negotiate_capabilities(&[Capability::Vision], &m);
    assert_eq!(r.unsupported.len(), 1);
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
    assert_eq!(
        r.native,
        vec![
            Capability::ToolRead,
            Capability::Streaming,
            Capability::ToolWrite
        ]
    );
}

#[test]
fn negotiate_capabilities_duplicates() {
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
// negotiate (CapabilityRequirements) tests
// ===========================================================================

#[test]
fn negotiate_reqs_all_native() {
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = require_native(&[Capability::Streaming]);
    let res = negotiate(&m, &r);
    assert_eq!(res.native, vec![Capability::Streaming]);
    assert!(res.is_compatible());
}

#[test]
fn negotiate_reqs_native_req_emulated_manifest() {
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
    let r = require_native(&[Capability::Streaming]);
    let res = negotiate(&m, &r);
    // Emulated doesn't satisfy Native minimum
    assert!(!res.is_compatible());
}

#[test]
fn negotiate_reqs_emulated_req_native_manifest() {
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = require_emulated(&[Capability::Streaming]);
    let res = negotiate(&m, &r);
    assert!(res.is_compatible());
    assert_eq!(res.native, vec![Capability::Streaming]);
}

#[test]
fn negotiate_reqs_emulated_req_emulated_manifest() {
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
    let r = require_emulated(&[Capability::Streaming]);
    let res = negotiate(&m, &r);
    assert!(res.is_compatible());
    assert_eq!(res.emulated.len(), 1);
}

#[test]
fn negotiate_reqs_empty_requirements() {
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = CapabilityRequirements::default();
    let res = negotiate(&m, &r);
    assert!(res.is_compatible());
    assert_eq!(res.total(), 0);
}

#[test]
fn negotiate_reqs_empty_manifest() {
    let r = require_native(&[Capability::Streaming]);
    let res = negotiate(&BTreeMap::new(), &r);
    assert!(!res.is_compatible());
}

#[test]
fn negotiate_reqs_both_empty() {
    let res = negotiate(&BTreeMap::new(), &CapabilityRequirements::default());
    assert!(res.is_compatible());
    assert_eq!(res.total(), 0);
}

#[test]
fn negotiate_reqs_mixed() {
    let m = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Emulated),
    ]);
    let r = require_emulated(&[
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
    ]);
    let res = negotiate(&m, &r);
    assert_eq!(res.native, vec![Capability::Streaming]);
    assert_eq!(res.emulated_caps(), vec![Capability::ToolRead]);
    assert_eq!(res.unsupported_caps(), vec![Capability::ToolWrite]);
}

#[test]
fn negotiate_reqs_restricted_satisfies_emulated() {
    let m = manifest_from(&[(
        Capability::ToolBash,
        CoreSupportLevel::Restricted {
            reason: "sandbox".into(),
        },
    )]);
    let r = require_emulated(&[Capability::ToolBash]);
    let res = negotiate(&m, &r);
    assert!(res.is_compatible());
    assert_eq!(res.emulated.len(), 1);
}

// ===========================================================================
// default_emulation_strategy tests
// ===========================================================================

#[test]
fn default_strategy_client_side_capabilities() {
    let client_side_caps = vec![
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
            "Expected ClientSide for {:?}",
            cap
        );
    }
}

#[test]
fn default_strategy_server_fallback_capabilities() {
    let server_fallback_caps = vec![
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
    for cap in &server_fallback_caps {
        assert_eq!(
            default_emulation_strategy(cap),
            EmulationStrategy::ServerFallback,
            "Expected ServerFallback for {:?}",
            cap
        );
    }
}

#[test]
fn default_strategy_approximate_capabilities() {
    let approximate_caps = vec![
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
    for cap in &approximate_caps {
        assert_eq!(
            default_emulation_strategy(cap),
            EmulationStrategy::Approximate,
            "Expected Approximate for {:?}",
            cap
        );
    }
}

#[test]
fn default_strategy_covers_all_capabilities() {
    // Ensures every variant has a strategy (no panic)
    for cap in all_capabilities() {
        let _ = default_emulation_strategy(&cap);
    }
}

// ===========================================================================
// generate_report tests
// ===========================================================================

#[test]
fn generate_report_all_native() {
    let r = NegotiationResult::from_simple(
        vec![Capability::Streaming, Capability::ToolUse],
        vec![],
        vec![],
    );
    let report = generate_report(&r);
    assert!(report.compatible);
    assert_eq!(report.native_count, 2);
    assert_eq!(report.details.len(), 2);
}

#[test]
fn generate_report_all_unsupported() {
    let r = NegotiationResult::from_simple(
        vec![],
        vec![],
        vec![Capability::Streaming, Capability::Audio],
    );
    let report = generate_report(&r);
    assert!(!report.compatible);
    assert_eq!(report.unsupported_count, 2);
}

#[test]
fn generate_report_all_emulated_still_compatible() {
    let r = NegotiationResult::from_simple(
        vec![],
        vec![Capability::Streaming, Capability::ToolRead],
        vec![],
    );
    let report = generate_report(&r);
    assert!(report.compatible);
    assert_eq!(report.emulated_count, 2);
    assert!(report.summary.contains("fully compatible"));
}

// ===========================================================================
// CapabilityRegistry tests
// ===========================================================================

#[test]
fn registry_new_is_empty() {
    let reg = CapabilityRegistry::new();
    assert!(reg.is_empty());
    assert_eq!(reg.len(), 0);
    assert!(reg.names().is_empty());
}

#[test]
fn registry_default_is_empty() {
    let reg = CapabilityRegistry::default();
    assert!(reg.is_empty());
}

#[test]
fn registry_register_and_get() {
    let mut reg = CapabilityRegistry::new();
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    reg.register("test", m);
    assert!(!reg.is_empty());
    assert_eq!(reg.len(), 1);
    assert!(reg.get("test").is_some());
    assert!(reg.contains("test"));
}

#[test]
fn registry_get_missing() {
    let reg = CapabilityRegistry::new();
    assert!(reg.get("missing").is_none());
    assert!(!reg.contains("missing"));
}

#[test]
fn registry_names_sorted() {
    let mut reg = CapabilityRegistry::new();
    reg.register("b", BTreeMap::new());
    reg.register("a", BTreeMap::new());
    reg.register("c", BTreeMap::new());
    // BTreeMap keeps keys sorted
    assert_eq!(reg.names(), vec!["a", "b", "c"]);
}

#[test]
fn registry_overwrite() {
    let mut reg = CapabilityRegistry::new();
    let m1 = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let m2 = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
    reg.register("x", m1);
    reg.register("x", m2);
    assert_eq!(reg.len(), 1);
    let got = reg.get("x").unwrap();
    assert!(matches!(
        got.get(&Capability::Streaming),
        Some(CoreSupportLevel::Emulated)
    ));
}

#[test]
fn registry_unregister_existing() {
    let mut reg = CapabilityRegistry::new();
    reg.register("a", BTreeMap::new());
    assert!(reg.unregister("a"));
    assert!(reg.is_empty());
}

#[test]
fn registry_unregister_nonexistent() {
    let mut reg = CapabilityRegistry::new();
    assert!(!reg.unregister("a"));
}

#[test]
fn registry_unregister_twice() {
    let mut reg = CapabilityRegistry::new();
    reg.register("a", BTreeMap::new());
    assert!(reg.unregister("a"));
    assert!(!reg.unregister("a"));
}

#[test]
fn registry_negotiate_by_name_exists() {
    let mut reg = CapabilityRegistry::new();
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    reg.register("b", m);
    let res = reg.negotiate_by_name("b", &[Capability::Streaming]);
    assert!(res.is_some());
    assert!(res.unwrap().is_compatible());
}

#[test]
fn registry_negotiate_by_name_missing() {
    let reg = CapabilityRegistry::new();
    assert!(
        reg.negotiate_by_name("nope", &[Capability::Streaming])
            .is_none()
    );
}

#[test]
fn registry_negotiate_by_name_unsupported_cap() {
    let mut reg = CapabilityRegistry::new();
    reg.register("b", BTreeMap::new());
    let res = reg
        .negotiate_by_name("b", &[Capability::Streaming])
        .unwrap();
    assert!(!res.is_compatible());
}

#[test]
fn registry_query_capability_empty() {
    let reg = CapabilityRegistry::new();
    let results = reg.query_capability(&Capability::Streaming);
    assert!(results.is_empty());
}

#[test]
fn registry_query_capability_multiple() {
    let mut reg = CapabilityRegistry::new();
    reg.register(
        "a",
        manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]),
    );
    reg.register(
        "b",
        manifest_from(&[(Capability::Streaming, CoreSupportLevel::Emulated)]),
    );
    let results = reg.query_capability(&Capability::Streaming);
    assert_eq!(results.len(), 2);
}

#[test]
fn registry_compare_both_exist() {
    let mut reg = CapabilityRegistry::new();
    reg.register(
        "a",
        manifest_from(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::Vision, CoreSupportLevel::Native),
        ]),
    );
    reg.register(
        "b",
        manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]),
    );
    let result = reg.compare("a", "b").unwrap();
    // a has Streaming + Vision (both non-Unsupported), b only has Streaming
    assert!(result.native.contains(&Capability::Streaming));
    assert!(!result.is_viable()); // Vision unsupported in b
}

#[test]
fn registry_compare_missing_source() {
    let reg = CapabilityRegistry::new();
    assert!(reg.compare("nope", "also-nope").is_none());
}

#[test]
fn registry_compare_missing_target() {
    let mut reg = CapabilityRegistry::new();
    reg.register("a", BTreeMap::new());
    assert!(reg.compare("a", "nope").is_none());
}

#[test]
fn registry_compare_same() {
    let mut reg = CapabilityRegistry::new();
    reg.register(
        "a",
        manifest_from(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolUse, CoreSupportLevel::Native),
        ]),
    );
    let result = reg.compare("a", "a").unwrap();
    assert!(result.is_viable());
}

#[test]
fn registry_clone() {
    let mut reg = CapabilityRegistry::new();
    reg.register("a", BTreeMap::new());
    let cloned = reg.clone();
    assert_eq!(cloned.len(), 1);
    assert!(cloned.contains("a"));
}

#[test]
fn registry_debug() {
    let reg = CapabilityRegistry::new();
    let dbg = format!("{:?}", reg);
    assert!(dbg.contains("CapabilityRegistry"));
}

// ===========================================================================
// CapabilityRegistry::with_defaults tests
// ===========================================================================

#[test]
fn registry_with_defaults_has_six_backends() {
    let reg = CapabilityRegistry::with_defaults();
    assert_eq!(reg.len(), 6);
}

#[test]
fn registry_with_defaults_contains_openai() {
    let reg = CapabilityRegistry::with_defaults();
    assert!(reg.contains("openai/gpt-4o"));
}

#[test]
fn registry_with_defaults_contains_claude() {
    let reg = CapabilityRegistry::with_defaults();
    assert!(reg.contains("anthropic/claude-3.5-sonnet"));
}

#[test]
fn registry_with_defaults_contains_gemini() {
    let reg = CapabilityRegistry::with_defaults();
    assert!(reg.contains("google/gemini-1.5-pro"));
}

#[test]
fn registry_with_defaults_contains_kimi() {
    let reg = CapabilityRegistry::with_defaults();
    assert!(reg.contains("moonshot/kimi"));
}

#[test]
fn registry_with_defaults_contains_codex() {
    let reg = CapabilityRegistry::with_defaults();
    assert!(reg.contains("openai/codex"));
}

#[test]
fn registry_with_defaults_contains_copilot() {
    let reg = CapabilityRegistry::with_defaults();
    assert!(reg.contains("github/copilot"));
}

#[test]
fn registry_with_defaults_all_support_streaming() {
    let reg = CapabilityRegistry::with_defaults();
    let results = reg.query_capability(&Capability::Streaming);
    assert!(
        results
            .iter()
            .all(|(_, level)| matches!(level, SupportLevel::Native))
    );
}

// ===========================================================================
// Pre-populated manifest tests
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
fn openai_manifest_vision_native() {
    let m = openai_gpt4o_manifest();
    assert_eq!(
        check_capability(&m, &Capability::Vision),
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
fn claude_manifest_pdf_native() {
    let m = claude_35_sonnet_manifest();
    assert_eq!(
        check_capability(&m, &Capability::PdfInput),
        SupportLevel::Native
    );
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
fn gemini_manifest_audio_native() {
    let m = gemini_15_pro_manifest();
    assert_eq!(
        check_capability(&m, &Capability::Audio),
        SupportLevel::Native
    );
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
fn kimi_manifest_audio_unsupported() {
    let m = kimi_manifest();
    assert!(matches!(
        check_capability(&m, &Capability::Audio),
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
fn codex_manifest_tool_read_native() {
    let m = codex_manifest();
    assert_eq!(
        check_capability(&m, &Capability::ToolRead),
        SupportLevel::Native
    );
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
fn copilot_manifest_tool_ask_user_native() {
    let m = copilot_manifest();
    assert_eq!(
        check_capability(&m, &Capability::ToolAskUser),
        SupportLevel::Native
    );
}

#[test]
fn copilot_manifest_extended_thinking_unsupported() {
    let m = copilot_manifest();
    assert!(matches!(
        check_capability(&m, &Capability::ExtendedThinking),
        SupportLevel::Unsupported { .. }
    ));
}

// ===========================================================================
// Cross-model negotiation tests
// ===========================================================================

#[test]
fn cross_model_all_support_streaming() {
    let required = &[Capability::Streaming];
    for m_fn in [
        openai_gpt4o_manifest,
        claude_35_sonnet_manifest,
        gemini_15_pro_manifest,
        kimi_manifest,
        codex_manifest,
        copilot_manifest,
    ] {
        let r = negotiate_capabilities(required, &m_fn());
        assert!(r.is_compatible(), "All models should support streaming");
    }
}

#[test]
fn cross_model_extended_thinking_only_claude() {
    let required = &[Capability::ExtendedThinking];
    assert!(negotiate_capabilities(required, &claude_35_sonnet_manifest()).is_compatible());
    assert!(!negotiate_capabilities(required, &openai_gpt4o_manifest()).is_compatible());
    assert!(!negotiate_capabilities(required, &gemini_15_pro_manifest()).is_compatible());
    assert!(!negotiate_capabilities(required, &kimi_manifest()).is_compatible());
    assert!(!negotiate_capabilities(required, &codex_manifest()).is_compatible());
    assert!(!negotiate_capabilities(required, &copilot_manifest()).is_compatible());
}

#[test]
fn cross_model_audio_openai_and_gemini() {
    let required = &[Capability::Audio];
    assert!(negotiate_capabilities(required, &openai_gpt4o_manifest()).is_compatible());
    assert!(negotiate_capabilities(required, &gemini_15_pro_manifest()).is_compatible());
    assert!(!negotiate_capabilities(required, &claude_35_sonnet_manifest()).is_compatible());
}

#[test]
fn cross_model_codex_vs_copilot_tools() {
    let required = &[
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolBash,
        Capability::ToolGlob,
        Capability::ToolGrep,
    ];
    assert!(negotiate_capabilities(required, &codex_manifest()).is_compatible());
    assert!(negotiate_capabilities(required, &copilot_manifest()).is_compatible());
}

// ===========================================================================
// NegotiationPolicy tests
// ===========================================================================

#[test]
fn negotiation_policy_default_is_strict() {
    assert_eq!(NegotiationPolicy::default(), NegotiationPolicy::Strict);
}

#[test]
fn negotiation_policy_display_strict() {
    assert_eq!(NegotiationPolicy::Strict.to_string(), "strict");
}

#[test]
fn negotiation_policy_display_best_effort() {
    assert_eq!(NegotiationPolicy::BestEffort.to_string(), "best-effort");
}

#[test]
fn negotiation_policy_display_permissive() {
    assert_eq!(NegotiationPolicy::Permissive.to_string(), "permissive");
}

#[test]
fn negotiation_policy_clone() {
    let p = NegotiationPolicy::BestEffort;
    assert_eq!(p.clone(), p);
}

#[test]
fn negotiation_policy_copy() {
    let p = NegotiationPolicy::Strict;
    let copied = p;
    assert_eq!(copied, p);
}

#[test]
fn negotiation_policy_eq() {
    assert_eq!(NegotiationPolicy::Strict, NegotiationPolicy::Strict);
    assert_ne!(NegotiationPolicy::Strict, NegotiationPolicy::Permissive);
}

#[test]
fn negotiation_policy_hash() {
    use std::collections::HashSet;
    let mut set = HashSet::new();
    set.insert(NegotiationPolicy::Strict);
    set.insert(NegotiationPolicy::Strict);
    assert_eq!(set.len(), 1);
    set.insert(NegotiationPolicy::BestEffort);
    set.insert(NegotiationPolicy::Permissive);
    assert_eq!(set.len(), 3);
}

#[test]
fn negotiation_policy_debug() {
    let dbg = format!("{:?}", NegotiationPolicy::Strict);
    assert!(dbg.contains("Strict"));
}

#[test]
fn negotiation_policy_serde_roundtrip_strict() {
    let p = NegotiationPolicy::Strict;
    let json = serde_json::to_string(&p).unwrap();
    let back: NegotiationPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(back, p);
}

#[test]
fn negotiation_policy_serde_roundtrip_best_effort() {
    let p = NegotiationPolicy::BestEffort;
    let json = serde_json::to_string(&p).unwrap();
    let back: NegotiationPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(back, p);
}

#[test]
fn negotiation_policy_serde_roundtrip_permissive() {
    let p = NegotiationPolicy::Permissive;
    let json = serde_json::to_string(&p).unwrap();
    let back: NegotiationPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(back, p);
}

#[test]
fn negotiation_policy_serde_json_values() {
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

// ===========================================================================
// NegotiationError tests
// ===========================================================================

#[test]
fn negotiation_error_display_single_unsupported() {
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
fn negotiation_error_display_multiple_unsupported() {
    let err = NegotiationError {
        policy: NegotiationPolicy::BestEffort,
        unsupported: vec![
            (Capability::Vision, "x".into()),
            (Capability::Audio, "y".into()),
            (Capability::Logprobs, "z".into()),
        ],
        warnings: vec![],
    };
    let msg = err.to_string();
    assert!(msg.contains("3 unsupported"));
    assert!(msg.contains("Vision"));
    assert!(msg.contains("Audio"));
    assert!(msg.contains("Logprobs"));
}

#[test]
fn negotiation_error_display_zero_unsupported() {
    let err = NegotiationError {
        policy: NegotiationPolicy::Strict,
        unsupported: vec![],
        warnings: vec![],
    };
    let msg = err.to_string();
    assert!(msg.contains("0 unsupported"));
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
fn negotiation_error_clone() {
    let err = NegotiationError {
        policy: NegotiationPolicy::Strict,
        unsupported: vec![(Capability::Vision, "x".into())],
        warnings: vec![Capability::Audio],
    };
    let cloned = err.clone();
    assert_eq!(cloned, err);
}

#[test]
fn negotiation_error_debug() {
    let err = NegotiationError {
        policy: NegotiationPolicy::Strict,
        unsupported: vec![],
        warnings: vec![],
    };
    let dbg = format!("{:?}", err);
    assert!(dbg.contains("NegotiationError"));
}

// ===========================================================================
// pre_negotiate tests
// ===========================================================================

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
fn pre_negotiate_empty_required() {
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = pre_negotiate(&[], &m);
    assert!(r.is_viable());
    assert_eq!(r.total(), 0);
}

#[test]
fn pre_negotiate_empty_manifest() {
    let r = pre_negotiate(&[Capability::Streaming], &BTreeMap::new());
    assert!(!r.is_viable());
}

// ===========================================================================
// apply_policy tests
// ===========================================================================

#[test]
fn apply_policy_strict_passes_all_native() {
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
fn apply_policy_best_effort_passes_emulated() {
    let m = manifest_from(&[(Capability::Vision, CoreSupportLevel::Emulated)]);
    let r = pre_negotiate(&[Capability::Vision], &m);
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
fn apply_policy_permissive_empty() {
    let r = pre_negotiate(&[], &BTreeMap::new());
    assert!(apply_policy(&r, NegotiationPolicy::Permissive).is_ok());
}

#[test]
fn apply_policy_strict_empty_requirements() {
    let r = pre_negotiate(&[], &BTreeMap::new());
    assert!(apply_policy(&r, NegotiationPolicy::Strict).is_ok());
}

// ===========================================================================
// Edge cases: BTreeMap / deterministic ordering
// ===========================================================================

#[test]
fn btreemap_manifest_deterministic_serialization() {
    let m = manifest_from(&[
        (Capability::ToolWrite, CoreSupportLevel::Native),
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Native),
    ]);
    let json1 = serde_json::to_string(&m).unwrap();
    let json2 = serde_json::to_string(&m).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn btreemap_manifest_from_iterator() {
    let entries = vec![
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Emulated),
    ];
    let m: CapabilityManifest = entries.into_iter().collect();
    assert_eq!(m.len(), 2);
}

// ===========================================================================
// Boundary / edge case tests
// ===========================================================================

#[test]
fn negotiate_all_capability_variants_against_empty_manifest() {
    let caps = all_capabilities();
    let r = negotiate_capabilities(&caps, &BTreeMap::new());
    assert_eq!(r.unsupported.len(), caps.len());
    assert!(!r.is_compatible());
}

#[test]
fn negotiate_all_capability_variants_all_native() {
    let caps = all_capabilities();
    let m: CapabilityManifest = caps
        .iter()
        .map(|c| (c.clone(), CoreSupportLevel::Native))
        .collect();
    let r = negotiate_capabilities(&caps, &m);
    assert_eq!(r.native.len(), caps.len());
    assert!(r.is_compatible());
}

#[test]
fn negotiate_all_capability_variants_all_emulated() {
    let caps = all_capabilities();
    let m: CapabilityManifest = caps
        .iter()
        .map(|c| (c.clone(), CoreSupportLevel::Emulated))
        .collect();
    let r = negotiate_capabilities(&caps, &m);
    assert_eq!(r.emulated.len(), caps.len());
    assert!(r.is_compatible());
}

#[test]
fn negotiate_single_capability_native() {
    for cap in all_capabilities() {
        let m = manifest_from(&[(cap.clone(), CoreSupportLevel::Native)]);
        let r = negotiate_capabilities(&[cap], &m);
        assert_eq!(r.native.len(), 1);
        assert!(r.is_compatible());
    }
}

#[test]
fn report_with_many_capabilities() {
    let caps = all_capabilities();
    let m: CapabilityManifest = caps
        .iter()
        .map(|c| (c.clone(), CoreSupportLevel::Native))
        .collect();
    let r = negotiate_capabilities(&caps, &m);
    let report = generate_report(&r);
    assert!(report.compatible);
    assert_eq!(report.native_count, caps.len());
    assert_eq!(report.details.len(), caps.len());
}

#[test]
fn registry_register_empty_name() {
    let mut reg = CapabilityRegistry::new();
    reg.register("", BTreeMap::new());
    assert!(reg.contains(""));
    assert_eq!(reg.len(), 1);
}

#[test]
fn registry_register_unicode_name() {
    let mut reg = CapabilityRegistry::new();
    reg.register("模型/gpt-4o", BTreeMap::new());
    assert!(reg.contains("模型/gpt-4o"));
}

#[test]
fn registry_register_special_chars_name() {
    let mut reg = CapabilityRegistry::new();
    reg.register("a/b-c_d.e", BTreeMap::new());
    assert!(reg.contains("a/b-c_d.e"));
}

#[test]
fn registry_compare_excludes_unsupported_source_caps() {
    let mut reg = CapabilityRegistry::new();
    reg.register(
        "source",
        manifest_from(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::Vision, CoreSupportLevel::Unsupported),
        ]),
    );
    reg.register(
        "target",
        manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]),
    );
    let result = reg.compare("source", "target").unwrap();
    // Vision is Unsupported in source, so it shouldn't be required from target
    assert!(result.is_viable());
    assert_eq!(result.native, vec![Capability::Streaming]);
}

#[test]
fn negotiation_result_serde_roundtrip_large() {
    let r = NegotiationResult {
        native: vec![
            Capability::Streaming,
            Capability::ToolUse,
            Capability::FunctionCalling,
        ],
        emulated: vec![
            (Capability::Vision, EmulationStrategy::Approximate),
            (Capability::ToolRead, EmulationStrategy::ClientSide),
            (
                Capability::ExtendedThinking,
                EmulationStrategy::ServerFallback,
            ),
        ],
        unsupported: vec![
            (Capability::Audio, "not available".into()),
            (Capability::Logprobs, "no API support".into()),
        ],
    };
    let json = serde_json::to_string_pretty(&r).unwrap();
    let back: NegotiationResult = serde_json::from_str(&json).unwrap();
    assert_eq!(back, r);
}

#[test]
fn compatibility_report_serde_roundtrip_incompatible() {
    let r = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::Vision],
        vec![Capability::Audio, Capability::Logprobs],
    );
    let report = generate_report(&r);
    assert!(!report.compatible);
    let json = serde_json::to_string(&report).unwrap();
    let back: CompatibilityReport = serde_json::from_str(&json).unwrap();
    assert_eq!(back, report);
}

#[test]
fn support_level_restricted_long_reason() {
    let long_reason = "a".repeat(10000);
    let s = SupportLevel::Restricted {
        reason: long_reason.clone(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let back: SupportLevel = serde_json::from_str(&json).unwrap();
    assert_eq!(back, s);
}

#[test]
fn negotiation_error_with_warnings() {
    let err = NegotiationError {
        policy: NegotiationPolicy::BestEffort,
        unsupported: vec![(Capability::Audio, "x".into())],
        warnings: vec![Capability::Vision, Capability::Streaming],
    };
    assert_eq!(err.warnings.len(), 2);
    assert_eq!(err.unsupported.len(), 1);
}
