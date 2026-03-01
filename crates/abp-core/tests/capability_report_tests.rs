// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for dialect-aware capability negotiation: DialectSupportLevel,
//! CapabilityReport, check_capabilities, and receipt metadata integration.

use abp_core::negotiate::{
    CapabilityReport, CapabilityReportEntry, DialectSupportLevel, check_capabilities,
    dialect_manifest,
};
use abp_core::{
    Capability, CapabilityRequirement, CapabilityRequirements, MinSupport, WorkOrderBuilder,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn work_order_with_caps(caps: Vec<Capability>) -> abp_core::WorkOrder {
    let reqs = CapabilityRequirements {
        required: caps
            .into_iter()
            .map(|c| CapabilityRequirement {
                capability: c,
                min_support: MinSupport::Emulated,
            })
            .collect(),
    };
    WorkOrderBuilder::new("test task")
        .requirements(reqs)
        .build()
}

// ---------------------------------------------------------------------------
// 1. DialectSupportLevel basics
// ---------------------------------------------------------------------------

#[test]
fn dialect_support_level_native_eq() {
    assert_eq!(DialectSupportLevel::Native, DialectSupportLevel::Native);
}

#[test]
fn dialect_support_level_emulated_carries_detail() {
    let level = DialectSupportLevel::Emulated {
        detail: "via polyfill".into(),
    };
    if let DialectSupportLevel::Emulated { detail } = &level {
        assert_eq!(detail, "via polyfill");
    } else {
        panic!("expected Emulated");
    }
}

#[test]
fn dialect_support_level_unsupported_carries_reason() {
    let level = DialectSupportLevel::Unsupported {
        reason: "not available".into(),
    };
    if let DialectSupportLevel::Unsupported { reason } = &level {
        assert_eq!(reason, "not available");
    } else {
        panic!("expected Unsupported");
    }
}

#[test]
fn dialect_support_level_serde_round_trip() {
    let levels = vec![
        DialectSupportLevel::Native,
        DialectSupportLevel::Emulated {
            detail: "adapter".into(),
        },
        DialectSupportLevel::Unsupported {
            reason: "missing".into(),
        },
    ];
    for level in &levels {
        let json = serde_json::to_string(level).unwrap();
        let back: DialectSupportLevel = serde_json::from_str(&json).unwrap();
        assert_eq!(*level, back);
    }
}

// ---------------------------------------------------------------------------
// 2. Dialect manifests — known dialect support levels
// ---------------------------------------------------------------------------

#[test]
fn claude_streaming_is_native() {
    let m = dialect_manifest("claude");
    assert_eq!(
        m.get(&Capability::Streaming),
        Some(&DialectSupportLevel::Native)
    );
}

#[test]
fn claude_logprobs_is_unsupported() {
    let m = dialect_manifest("claude");
    assert!(matches!(
        m.get(&Capability::Logprobs),
        Some(DialectSupportLevel::Unsupported { .. })
    ));
}

#[test]
fn openai_structured_output_is_native() {
    let m = dialect_manifest("openai");
    assert_eq!(
        m.get(&Capability::StructuredOutputJsonSchema),
        Some(&DialectSupportLevel::Native)
    );
}

#[test]
fn openai_extended_thinking_unsupported() {
    let m = dialect_manifest("openai");
    assert!(matches!(
        m.get(&Capability::ExtendedThinking),
        Some(DialectSupportLevel::Unsupported { .. })
    ));
}

#[test]
fn gemini_pdf_input_is_native() {
    let m = dialect_manifest("gemini");
    assert_eq!(
        m.get(&Capability::PdfInput),
        Some(&DialectSupportLevel::Native)
    );
}

#[test]
fn gemini_extended_thinking_is_emulated() {
    let m = dialect_manifest("gemini");
    assert!(matches!(
        m.get(&Capability::ExtendedThinking),
        Some(DialectSupportLevel::Emulated { .. })
    ));
}

#[test]
fn unknown_dialect_returns_empty_manifest() {
    let m = dialect_manifest("unknown_dialect");
    assert!(m.is_empty());
}

// ---------------------------------------------------------------------------
// 3. check_capabilities — pre-execution check
// ---------------------------------------------------------------------------

#[test]
fn check_capabilities_all_native() {
    let wo = work_order_with_caps(vec![Capability::Streaming, Capability::ToolUse]);
    let report = check_capabilities(&wo, "claude", "openai");
    assert!(report.all_satisfiable());
    assert_eq!(report.native_capabilities().len(), 2);
    assert!(report.emulated_capabilities().is_empty());
    assert!(report.unsupported_capabilities().is_empty());
}

#[test]
fn check_capabilities_mixed_support() {
    let wo = work_order_with_caps(vec![
        Capability::Streaming,
        Capability::ExtendedThinking,
        Capability::Logprobs,
    ]);
    let report = check_capabilities(&wo, "openai", "claude");
    // Streaming: native, ExtendedThinking: native, Logprobs: unsupported
    assert_eq!(report.native_capabilities().len(), 2);
    assert_eq!(report.unsupported_capabilities().len(), 1);
    assert!(!report.all_satisfiable());
}

#[test]
fn check_capabilities_emulated_entries() {
    let wo = work_order_with_caps(vec![Capability::CodeExecution]);
    let report = check_capabilities(&wo, "openai", "claude");
    assert_eq!(report.emulated_capabilities().len(), 1);
    assert!(report.all_satisfiable());
}

#[test]
fn check_capabilities_unknown_target_dialect() {
    let wo = work_order_with_caps(vec![Capability::Streaming]);
    let report = check_capabilities(&wo, "claude", "unknown");
    assert!(!report.all_satisfiable());
    assert_eq!(report.unsupported_capabilities().len(), 1);
}

#[test]
fn check_capabilities_empty_requirements() {
    let wo = WorkOrderBuilder::new("no caps needed").build();
    let report = check_capabilities(&wo, "claude", "openai");
    assert!(report.entries.is_empty());
    assert!(report.all_satisfiable());
}

#[test]
fn check_capabilities_preserves_dialect_names() {
    let wo = work_order_with_caps(vec![Capability::Streaming]);
    let report = check_capabilities(&wo, "claude", "gemini");
    assert_eq!(report.source_dialect, "claude");
    assert_eq!(report.target_dialect, "gemini");
}

// ---------------------------------------------------------------------------
// 4. CapabilityReport helpers
// ---------------------------------------------------------------------------

#[test]
fn capability_report_categorization() {
    let report = CapabilityReport {
        source_dialect: "test_src".into(),
        target_dialect: "test_tgt".into(),
        entries: vec![
            CapabilityReportEntry {
                capability: Capability::Streaming,
                support: DialectSupportLevel::Native,
            },
            CapabilityReportEntry {
                capability: Capability::CodeExecution,
                support: DialectSupportLevel::Emulated {
                    detail: "via tool_bash".into(),
                },
            },
            CapabilityReportEntry {
                capability: Capability::Logprobs,
                support: DialectSupportLevel::Unsupported {
                    reason: "not available".into(),
                },
            },
        ],
    };
    assert_eq!(report.native_capabilities().len(), 1);
    assert_eq!(report.emulated_capabilities().len(), 1);
    assert_eq!(report.unsupported_capabilities().len(), 1);
    assert!(!report.all_satisfiable());
}

// ---------------------------------------------------------------------------
// 5. Receipt metadata integration
// ---------------------------------------------------------------------------

#[test]
fn capability_report_to_receipt_metadata() {
    let wo = work_order_with_caps(vec![Capability::Streaming, Capability::ToolUse]);
    let report = check_capabilities(&wo, "claude", "openai");
    let metadata = report.to_receipt_metadata();
    assert!(metadata.is_object());
    let obj = metadata.as_object().unwrap();
    assert!(obj.contains_key("source_dialect"));
    assert!(obj.contains_key("target_dialect"));
    assert!(obj.contains_key("entries"));
    assert_eq!(obj["source_dialect"], "claude");
}

#[test]
fn capability_report_metadata_in_receipt() {
    use abp_core::{Outcome, ReceiptBuilder};

    let wo = work_order_with_caps(vec![Capability::Streaming]);
    let report = check_capabilities(&wo, "claude", "openai");
    let metadata = report.to_receipt_metadata();

    // Embed report in receipt's usage_raw as an example of integration
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .usage_raw(serde_json::json!({ "capability_report": metadata }))
        .build();

    let raw = &receipt.usage_raw;
    assert!(raw["capability_report"]["entries"].is_array());
}

// ---------------------------------------------------------------------------
// 6. Unknown capability handling
// ---------------------------------------------------------------------------

#[test]
fn unknown_capability_in_known_dialect() {
    // McpServer is not in the claude manifest — should report unsupported
    let wo = work_order_with_caps(vec![Capability::McpServer]);
    let report = check_capabilities(&wo, "openai", "claude");
    assert_eq!(report.unsupported_capabilities().len(), 1);
    let entry = &report.entries[0];
    if let DialectSupportLevel::Unsupported { reason } = &entry.support {
        assert!(reason.contains("not recognized"));
    } else {
        panic!("expected Unsupported for McpServer in claude dialect");
    }
}

#[test]
fn multiple_unknown_capabilities() {
    let wo = work_order_with_caps(vec![
        Capability::McpClient,
        Capability::McpServer,
        Capability::SessionFork,
    ]);
    let report = check_capabilities(&wo, "claude", "openai");
    // None of these are in the openai manifest
    assert_eq!(report.unsupported_capabilities().len(), 3);
    assert!(!report.all_satisfiable());
}

// ---------------------------------------------------------------------------
// 7. Cross-dialect comparison
// ---------------------------------------------------------------------------

#[test]
fn claude_vs_openai_extended_thinking() {
    let wo = work_order_with_caps(vec![Capability::ExtendedThinking]);

    let to_claude = check_capabilities(&wo, "openai", "claude");
    assert!(to_claude.all_satisfiable());
    assert_eq!(to_claude.native_capabilities().len(), 1);

    let to_openai = check_capabilities(&wo, "claude", "openai");
    assert!(!to_openai.all_satisfiable());
    assert_eq!(to_openai.unsupported_capabilities().len(), 1);
}

#[test]
fn openai_vs_gemini_logprobs() {
    let wo = work_order_with_caps(vec![Capability::Logprobs]);

    let to_openai = check_capabilities(&wo, "gemini", "openai");
    assert!(to_openai.all_satisfiable());
    assert_eq!(to_openai.native_capabilities().len(), 1);

    let to_gemini = check_capabilities(&wo, "openai", "gemini");
    assert!(!to_gemini.all_satisfiable());
}
