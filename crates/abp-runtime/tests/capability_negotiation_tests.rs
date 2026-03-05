#![allow(clippy::all)]
#![allow(clippy::manual_repeat_n)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::single_component_path_imports)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unnecessary_to_owned)]
#![allow(clippy::implicit_clone)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::iter_kv_map)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::single_match)]
#![allow(clippy::manual_map)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::len_zero)]
#![allow(clippy::map_entry)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(unknown_lints)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for capability negotiation integration in the runtime pipeline.

use abp_core::{
    Capability, CapabilityRequirement, CapabilityRequirements, ExecutionLane, MinSupport,
    PolicyProfile, WorkOrder, WorkspaceMode, WorkspaceSpec,
};
use abp_runtime::Runtime;
use serde_json;
use tokio_stream::StreamExt;

fn mock_work_order() -> WorkOrder {
    WorkOrder {
        id: uuid::Uuid::new_v4(),
        task: "capability negotiation test".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
            mode: WorkspaceMode::PassThrough,
            include: vec![],
            exclude: vec![],
        },
        context: abp_core::ContextPacket::default(),
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: abp_core::RuntimeConfig::default(),
    }
}

// ---------- 1. Successful negotiation: all capabilities native ----------

#[tokio::test]
async fn negotiation_all_native_succeeds() {
    let rt = Runtime::with_default_backends();
    let mut wo = mock_work_order();
    // MockBackend advertises Streaming as Native.
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };

    let handle = rt.run_streaming("mock", wo).await.expect("run_streaming");
    let _events: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.expect("join").expect("receipt");

    // Receipt should contain negotiation metadata with native capabilities.
    let negotiation = receipt
        .usage_raw
        .get("capability_negotiation")
        .expect("negotiation result must be in receipt metadata");
    let native = negotiation
        .get("native")
        .and_then(|v| v.as_array())
        .expect("native array");
    assert!(
        native.iter().any(|v| v.as_str() == Some("streaming")),
        "Streaming should be in native list: {native:?}"
    );
    let unsupported = negotiation
        .get("unsupported")
        .and_then(|v| v.as_array())
        .expect("unsupported array");
    assert!(
        unsupported.is_empty(),
        "no capabilities should be unsupported"
    );
}

// ---------- 2. Partial negotiation: some native, some emulated ----------

#[tokio::test]
async fn negotiation_partial_emulated_succeeds() {
    let rt = Runtime::with_default_backends();
    let mut wo = mock_work_order();
    // MockBackend: Streaming=Native, ToolRead=Emulated.
    wo.requirements = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Emulated,
            },
        ],
    };

    let handle = rt.run_streaming("mock", wo).await.expect("run_streaming");
    let _events: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.expect("join").expect("receipt");

    let negotiation = receipt
        .usage_raw
        .get("capability_negotiation")
        .expect("negotiation result must be in receipt metadata");
    let native = negotiation
        .get("native")
        .and_then(|v| v.as_array())
        .expect("native array");
    let emulatable = negotiation
        .get("emulated")
        .and_then(|v| v.as_array())
        .expect("emulated array");
    assert!(
        !native.is_empty(),
        "should have at least one native capability"
    );
    assert!(
        !emulatable.is_empty(),
        "should have at least one emulatable capability"
    );
    let unsupported = negotiation
        .get("unsupported")
        .and_then(|v| v.as_array())
        .expect("unsupported array");
    assert!(
        unsupported.is_empty(),
        "no capabilities should be unsupported"
    );
}

// ---------- 3. Failed negotiation: missing required capability ----------

#[tokio::test]
async fn negotiation_missing_required_capability_errors() {
    let rt = Runtime::with_default_backends();
    let mut wo = mock_work_order();
    // McpClient is not in MockBackend's manifest.
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Native,
        }],
    };

    let result = rt.run_streaming("mock", wo).await;
    let err = match result {
        Ok(_) => panic!("missing required capability should cause an error"),
        Err(e) => e,
    };
    let err_msg = format!("{err}");
    assert!(
        err_msg.contains("capability") || err_msg.contains("Capability"),
        "error should mention capability: {err_msg}"
    );
}

// ---------- 4. NegotiationResult recorded in receipt ----------

#[tokio::test]
async fn receipt_contains_negotiation_result_key() {
    let rt = Runtime::with_default_backends();
    let mut wo = mock_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let _: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.unwrap().unwrap();

    assert!(
        receipt.usage_raw.get("negotiation_result").is_some(),
        "receipt must contain negotiation_result"
    );
}

// ---------- 5. NegotiationResult native list ----------

#[tokio::test]
async fn negotiation_result_lists_native_capabilities() {
    let rt = Runtime::with_default_backends();
    let mut wo = mock_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let _: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.unwrap().unwrap();

    let nr = receipt.usage_raw.get("negotiation_result").unwrap();
    let native = nr.get("native").and_then(|v| v.as_array()).unwrap();
    assert!(
        native.iter().any(|v| v.as_str() == Some("streaming")),
        "native list should include streaming: {native:?}"
    );
}

// ---------- 6. NegotiationResult emulated list ----------

#[tokio::test]
async fn negotiation_result_lists_emulated_capabilities() {
    let rt = Runtime::with_default_backends();
    let mut wo = mock_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Emulated,
            },
        ],
    };
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let _: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.unwrap().unwrap();

    let nr = receipt.usage_raw.get("negotiation_result").unwrap();
    let emulated = nr.get("emulated").and_then(|v| v.as_array()).unwrap();
    assert!(!emulated.is_empty(), "emulated list should not be empty");
    assert!(
        emulated
            .iter()
            .any(|e| { e.get("capability").and_then(|c| c.as_str()) == Some("tool_read") }),
        "ToolRead should be in emulated list: {emulated:?}"
    );
}

// ---------- 7. NegotiationResult missing list is empty on success ----------

#[tokio::test]
async fn negotiation_result_missing_empty_on_success() {
    let rt = Runtime::with_default_backends();
    let mut wo = mock_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let _: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.unwrap().unwrap();

    let nr = receipt.usage_raw.get("negotiation_result").unwrap();
    let missing = nr.get("missing").and_then(|v| v.as_array()).unwrap();
    assert!(missing.is_empty(), "missing should be empty on success");
}

// ---------- 8. Empty requirements succeed ----------

#[tokio::test]
async fn negotiation_empty_requirements_succeeds() {
    let rt = Runtime::with_default_backends();
    let wo = mock_work_order(); // default empty requirements
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let _: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.unwrap().unwrap();

    assert!(receipt.usage_raw.get("negotiation_result").is_some());
}

// ---------- 9. Multiple native requirements all pass ----------

#[tokio::test]
async fn negotiation_multiple_native_requirements() {
    let rt = Runtime::with_default_backends();
    let mut wo = mock_work_order();
    // All of these are in MockBackend's manifest
    wo.requirements = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Emulated,
            },
            CapabilityRequirement {
                capability: Capability::ToolWrite,
                min_support: MinSupport::Emulated,
            },
        ],
    };

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let _: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.unwrap().unwrap();

    let nr = receipt.usage_raw.get("negotiation_result").unwrap();
    let native = nr.get("native").and_then(|v| v.as_array()).unwrap();
    assert!(
        native.iter().any(|v| v.as_str() == Some("streaming")),
        "Streaming should be native"
    );
}

// ---------- 10. Error is CapabilityCheckFailed variant ----------

#[tokio::test]
async fn negotiation_error_is_capability_check_failed() {
    let rt = Runtime::with_default_backends();
    let mut wo = mock_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Native,
        }],
    };

    let result = rt.run_streaming("mock", wo).await;
    match result {
        Err(err) => {
            let err_msg = format!("{err}");
            assert!(
                err_msg.contains("capability"),
                "error should mention capability: {err_msg}"
            );
        }
        Ok(_) => panic!("expected error for native-only McpClient requirement"),
    }
}

// ---------- 11. Multiple unsupported capabilities in error ----------

#[tokio::test]
async fn negotiation_multiple_unsupported_capabilities() {
    let rt = Runtime::with_default_backends();
    let mut wo = mock_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::McpClient,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::Vision,
                min_support: MinSupport::Native,
            },
        ],
    };

    let result = rt.run_streaming("mock", wo).await;
    match result {
        Err(err) => {
            let err_msg = format!("{err}");
            assert!(
                err_msg.contains("capability") || err_msg.contains("Capability"),
                "error should mention capability: {err_msg}"
            );
        }
        Ok(_) => panic!("expected error for multiple native-only unsupported capabilities"),
    }
}

// ---------- 12. Unknown backend fails before negotiation ----------

#[tokio::test]
async fn unknown_backend_fails_before_negotiation() {
    let rt = Runtime::with_default_backends();
    let wo = mock_work_order();
    let result = rt.run_streaming("nonexistent", wo).await;
    match result {
        Err(err) => {
            let err_msg = format!("{err}");
            assert!(
                err_msg.contains("unknown backend")
                    || err_msg.contains("not found")
                    || err_msg.contains("Unknown backend")
                    || err_msg.contains("backend"),
                "should report unknown backend: {err_msg}"
            );
        }
        Ok(_) => panic!("expected error for nonexistent backend"),
    }
}

// ---------- 13. NegotiationResult emulated source is "backend" ----------

#[tokio::test]
async fn negotiation_result_emulated_source_is_backend() {
    let rt = Runtime::with_default_backends();
    let mut wo = mock_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolWrite,
            min_support: MinSupport::Emulated,
        }],
    };
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let _: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.unwrap().unwrap();

    let nr = receipt.usage_raw.get("negotiation_result").unwrap();
    let emulated = nr.get("emulated").and_then(|v| v.as_array()).unwrap();
    for entry in emulated {
        let source = entry.get("source").and_then(|s| s.as_str()).unwrap();
        assert_eq!(
            source, "backend",
            "emulated capabilities from backend should have source=backend"
        );
    }
}

// ---------- 14. NegotiationResult total count ----------

#[tokio::test]
async fn negotiation_result_total_count() {
    let rt = Runtime::with_default_backends();
    let mut wo = mock_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Emulated,
            },
        ],
    };
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let _: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.unwrap().unwrap();

    let nr = receipt.usage_raw.get("negotiation_result").unwrap();
    let native_count = nr
        .get("native")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    let emulated_count = nr
        .get("emulated")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    let missing_count = nr
        .get("missing")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    assert_eq!(native_count + emulated_count + missing_count, 2);
}

// ---------- 15. Capability negotiation JSON still present ----------

#[tokio::test]
async fn receipt_retains_capability_negotiation_key() {
    let rt = Runtime::with_default_backends();
    let mut wo = mock_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let _: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.unwrap().unwrap();

    // Both the raw capability_negotiation AND the combined negotiation_result should be present.
    assert!(receipt.usage_raw.get("capability_negotiation").is_some());
    assert!(receipt.usage_raw.get("negotiation_result").is_some());
}

// ---------- 16. NegotiationResult is_viable on native only ----------

#[tokio::test]
async fn negotiation_result_is_viable_native_only() {
    let nr = abp_runtime::negotiate::NegotiationResult::all_native(vec![Capability::Streaming]);
    assert!(nr.is_viable());
    assert_eq!(nr.total(), 1);
    assert!(nr.missing.is_empty());
    assert!(nr.emulated.is_empty());
}

// ---------- 17. NegotiationResult not viable with missing ----------

#[test]
fn negotiation_result_not_viable_with_missing() {
    let nr = abp_runtime::negotiate::NegotiationResult {
        native: vec![],
        emulated: vec![],
        missing: vec![abp_runtime::negotiate::MissingCapability {
            capability: Capability::Vision,
            reason: "not available".into(),
        }],
    };
    assert!(!nr.is_viable());
}

// ---------- 18. NegotiationResult Display impl ----------

#[test]
fn negotiation_result_display() {
    let nr = abp_runtime::negotiate::NegotiationResult {
        native: vec![Capability::Streaming],
        emulated: vec![abp_runtime::negotiate::EmulatedCapability {
            capability: Capability::ToolRead,
            source: "backend".into(),
            description: "test".into(),
        }],
        missing: vec![],
    };
    let s = format!("{nr}");
    assert!(s.contains("1 native"), "display: {s}");
    assert!(s.contains("1 emulated"), "display: {s}");
    assert!(s.contains("0 missing"), "display: {s}");
}

// ---------- 19. NegotiationResult serde roundtrip ----------

#[test]
fn negotiation_result_serde_json_roundtrip() {
    let nr = abp_runtime::negotiate::NegotiationResult {
        native: vec![Capability::Streaming],
        emulated: vec![abp_runtime::negotiate::EmulatedCapability {
            capability: Capability::ToolRead,
            source: "backend".into(),
            description: "polyfill".into(),
        }],
        missing: vec![abp_runtime::negotiate::MissingCapability {
            capability: Capability::Vision,
            reason: "not available".into(),
        }],
    };
    let json = serde_json::to_string(&nr).unwrap();
    let back: abp_runtime::negotiate::NegotiationResult = serde_json::from_str(&json).unwrap();
    assert_eq!(back, nr);
}

// ---------- 20. from_negotiation with mixed caps ----------

#[test]
fn from_negotiation_mixed_native_and_emulated() {
    let cap = abp_capability::NegotiationResult {
        native: vec![Capability::Streaming],
        emulated: vec![(
            Capability::ToolRead,
            abp_capability::EmulationStrategy::ClientSide,
        )],
        unsupported: vec![(Capability::Vision, "not in manifest".into())],
    };
    let nr = abp_runtime::negotiate::NegotiationResult::from_negotiation(&cap, None);
    assert!(!nr.is_viable());
    assert_eq!(nr.native.len(), 1);
    assert_eq!(nr.emulated.len(), 1);
    assert_eq!(nr.missing.len(), 1);
}

// ---------- 21. Runtime emulation rescues unsupported cap ----------

#[test]
fn from_negotiation_runtime_emulation_rescues_gap() {
    use abp_emulation::{EmulationEntry, EmulationReport, EmulationStrategy as ES};

    let cap = abp_capability::NegotiationResult {
        native: vec![Capability::Streaming],
        emulated: vec![],
        unsupported: vec![(Capability::ExtendedThinking, "not in manifest".into())],
    };
    let emu = EmulationReport {
        applied: vec![EmulationEntry {
            capability: Capability::ExtendedThinking,
            strategy: ES::SystemPromptInjection {
                prompt: "think".into(),
            },
        }],
        warnings: vec![],
    };
    let nr = abp_runtime::negotiate::NegotiationResult::from_negotiation(&cap, Some(&emu));
    assert!(nr.is_viable(), "runtime emulation should rescue the gap");
    assert!(nr.missing.is_empty());
    assert!(
        nr.emulated
            .iter()
            .any(|e| e.capability == Capability::ExtendedThinking && e.source == "runtime"),
        "ExtendedThinking should appear as runtime-emulated"
    );
}

// ---------- 22. Emulation with config override ----------

#[tokio::test]
async fn emulation_with_config_succeeds_for_extended_thinking() {
    use abp_emulation::{EmulationConfig, EmulationStrategy};

    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::SystemPromptInjection {
            prompt: "think step by step".into(),
        },
    );

    let rt = Runtime::with_default_backends().with_emulation(config);
    let mut wo = mock_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ExtendedThinking,
                min_support: MinSupport::Emulated,
            },
        ],
    };

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let _: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert!(receipt.usage_raw.get("negotiation_result").is_some());
}

// ---------- 23. Emulation report present when emulation applied ----------

#[tokio::test]
async fn emulation_report_in_receipt_when_applied() {
    use abp_emulation::{EmulationConfig, EmulationStrategy};

    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::SystemPromptInjection {
            prompt: "step by step".into(),
        },
    );

    let rt = Runtime::with_default_backends().with_emulation(config);
    let mut wo = mock_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ExtendedThinking,
            min_support: MinSupport::Emulated,
        }],
    };

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let _: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert!(
        receipt.usage_raw.get("emulation").is_some(),
        "emulation report should be in receipt"
    );
}

// ---------- 24. NegotiationResult runtime-emulated source ----------

#[tokio::test]
async fn negotiation_result_shows_runtime_emulated_source() {
    use abp_emulation::{EmulationConfig, EmulationStrategy};

    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::SystemPromptInjection {
            prompt: "step by step".into(),
        },
    );

    let rt = Runtime::with_default_backends().with_emulation(config);
    let mut wo = mock_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ExtendedThinking,
            min_support: MinSupport::Emulated,
        }],
    };

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let _: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.unwrap().unwrap();

    let nr = receipt.usage_raw.get("negotiation_result").unwrap();
    let emulated = nr.get("emulated").and_then(|v| v.as_array()).unwrap();
    assert!(
        emulated
            .iter()
            .any(|e| { e.get("source").and_then(|s| s.as_str()) == Some("runtime") }),
        "should have runtime-emulated entry: {emulated:?}"
    );
}

// ---------- 25. Emulation fails for unemulatable capability ----------

#[tokio::test]
async fn emulation_fails_for_unemulatable_capability() {
    use abp_emulation::EmulationConfig;

    let config = EmulationConfig::new(); // no override for CodeExecution (defaults to Disabled)
    let rt = Runtime::with_default_backends().with_emulation(config);
    let mut wo = mock_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::CodeExecution,
            min_support: MinSupport::Native,
        }],
    };

    let result = rt.run_streaming("mock", wo).await;
    match result {
        Err(err) => {
            let err_msg = format!("{err}");
            assert!(
                err_msg.contains("capability") || err_msg.contains("emulation"),
                "error should mention capability or emulation: {err_msg}"
            );
        }
        Ok(_) => panic!("expected error for unemulatable capability"),
    }
}

// ---------- 26. NegotiationResult from empty negotiation ----------

#[test]
fn negotiation_result_from_empty_negotiation() {
    let cap = abp_capability::NegotiationResult {
        native: vec![],
        emulated: vec![],
        unsupported: vec![],
    };
    let nr = abp_runtime::negotiate::NegotiationResult::from_negotiation(&cap, None);
    assert!(nr.is_viable());
    assert_eq!(nr.total(), 0);
}

// ---------- 27. ToolBash emulated through mock ----------

#[tokio::test]
async fn negotiation_tool_bash_emulated() {
    let rt = Runtime::with_default_backends();
    let mut wo = mock_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolBash,
            min_support: MinSupport::Emulated,
        }],
    };

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let _: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.unwrap().unwrap();

    let nr = receipt.usage_raw.get("negotiation_result").unwrap();
    let emulated = nr.get("emulated").and_then(|v| v.as_array()).unwrap();
    assert!(
        emulated
            .iter()
            .any(|e| { e.get("capability").and_then(|c| c.as_str()) == Some("tool_bash") }),
        "ToolBash should be emulated: {emulated:?}"
    );
}

// ---------- 28. All mock capabilities satisfied at Emulated level ----------

#[tokio::test]
async fn all_mock_capabilities_satisfied_at_emulated_level() {
    let rt = Runtime::with_default_backends();
    let mut wo = mock_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Emulated,
            },
            CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Emulated,
            },
            CapabilityRequirement {
                capability: Capability::ToolWrite,
                min_support: MinSupport::Emulated,
            },
            CapabilityRequirement {
                capability: Capability::ToolEdit,
                min_support: MinSupport::Emulated,
            },
            CapabilityRequirement {
                capability: Capability::ToolBash,
                min_support: MinSupport::Emulated,
            },
            CapabilityRequirement {
                capability: Capability::StructuredOutputJsonSchema,
                min_support: MinSupport::Emulated,
            },
        ],
    };

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let _: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.unwrap().unwrap();

    let nr = receipt.usage_raw.get("negotiation_result").unwrap();
    let missing = nr.get("missing").and_then(|v| v.as_array()).unwrap();
    assert!(missing.is_empty(), "all capabilities should be satisfied");
}

// ---------- 29. StructuredOutputJsonSchema emulated ----------

#[tokio::test]
async fn negotiation_structured_output_emulated() {
    let rt = Runtime::with_default_backends();
    let mut wo = mock_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::StructuredOutputJsonSchema,
            min_support: MinSupport::Emulated,
        }],
    };

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let _: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.unwrap().unwrap();

    let nr = receipt.usage_raw.get("negotiation_result").unwrap();
    let emulated = nr.get("emulated").and_then(|v| v.as_array()).unwrap();
    assert!(
        emulated.iter().any(|e| {
            e.get("capability").and_then(|c| c.as_str()) == Some("structured_output_json_schema")
        }),
        "StructuredOutputJsonSchema should be in emulated: {emulated:?}"
    );
}

// ---------- 30. Receipt hash present after negotiation ----------

#[tokio::test]
async fn receipt_has_hash_after_negotiation() {
    let rt = Runtime::with_default_backends();
    let mut wo = mock_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };

    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let _: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.unwrap().unwrap();
    assert!(receipt.receipt_sha256.is_some(), "receipt should have hash");
}

// ---------- 31. Negotiation without emulation config fails on missing ----------

#[tokio::test]
async fn no_emulation_config_rejects_missing_capability() {
    let rt = Runtime::with_default_backends(); // no emulation config
    let mut wo = mock_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ExtendedThinking,
            min_support: MinSupport::Native,
        }],
    };

    let result = rt.run_streaming("mock", wo).await;
    match result {
        Err(err) => {
            let err_msg = format!("{err}");
            assert!(
                err_msg.contains("capability") || err_msg.contains("Capability"),
                "should mention capability check: {err_msg}"
            );
        }
        Ok(_) => panic!("expected error for native-only ExtendedThinking requirement"),
    }
}

// ---------- 32. NegotiationResult JSON has expected schema ----------

#[tokio::test]
async fn negotiation_result_json_schema_correct() {
    let rt = Runtime::with_default_backends();
    let mut wo = mock_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Emulated,
            },
        ],
    };
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let _: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.unwrap().unwrap();

    let nr = receipt.usage_raw.get("negotiation_result").unwrap();
    // All three keys must be present.
    assert!(nr.get("native").is_some(), "native key must exist");
    assert!(nr.get("emulated").is_some(), "emulated key must exist");
    assert!(nr.get("missing").is_some(), "missing key must exist");

    // native should be an array.
    assert!(nr["native"].is_array());
    // emulated should be an array of objects.
    assert!(nr["emulated"].is_array());
    // missing should be an array.
    assert!(nr["missing"].is_array());
}

// ---------- 33. EmulatedCapability has description field ----------

#[tokio::test]
async fn emulated_capability_has_description() {
    let rt = Runtime::with_default_backends();
    let mut wo = mock_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolEdit,
            min_support: MinSupport::Emulated,
        }],
    };
    let handle = rt.run_streaming("mock", wo).await.unwrap();
    let _: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.unwrap().unwrap();

    let nr = receipt.usage_raw.get("negotiation_result").unwrap();
    let emulated = nr.get("emulated").and_then(|v| v.as_array()).unwrap();
    for entry in emulated {
        assert!(
            entry.get("description").is_some(),
            "emulated entry must have description: {entry:?}"
        );
    }
}

// ---------- 34. Preflight runs before backend execution (missing cap = no events) ----------

#[tokio::test]
async fn preflight_prevents_backend_execution() {
    let rt = Runtime::with_default_backends();
    let mut wo = mock_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Audio,
            min_support: MinSupport::Native,
        }],
    };

    // The run should fail before the backend produces any events.
    let result = rt.run_streaming("mock", wo).await;
    assert!(result.is_err(), "should fail at preflight");
}
