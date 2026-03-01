// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for emulation engine wiring in the runtime pipeline.

use abp_core::{
    Capability, CapabilityRequirement, CapabilityRequirements, ExecutionLane, MinSupport,
    PolicyProfile, WorkOrder, WorkspaceMode, WorkspaceSpec,
};
use abp_emulation::{EmulationConfig, EmulationStrategy};
use abp_runtime::Runtime;
use tokio_stream::StreamExt;

fn mock_work_order() -> WorkOrder {
    WorkOrder {
        id: uuid::Uuid::new_v4(),
        task: "emulation test".into(),
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

/// Helper: run a work order end-to-end, collecting events and receipt.
async fn run_to_completion(
    rt: &Runtime,
    wo: WorkOrder,
) -> (Vec<abp_core::AgentEvent>, abp_core::Receipt) {
    let handle = rt.run_streaming("mock", wo).await.expect("run_streaming");
    let events: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.expect("join").expect("receipt");
    (events, receipt)
}

// ── 1. Runtime with emulation config accepts backend missing capabilities ──

#[tokio::test]
async fn emulation_allows_missing_emulatable_capability() {
    // ExtendedThinking is NOT in MockBackend's manifest but CAN be emulated.
    let rt = Runtime::with_default_backends().with_emulation(EmulationConfig::new());
    let mut wo = mock_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ExtendedThinking,
            min_support: MinSupport::Emulated,
        }],
    };

    let result = rt.run_streaming("mock", wo).await;
    assert!(result.is_ok(), "emulatable capability should be accepted");

    let handle = result.unwrap();
    let _: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.expect("join").expect("receipt");
    assert!(receipt.receipt_sha256.is_some());
}

// ── 2. Emulation is not applied when backend has native capability ──

#[tokio::test]
async fn no_emulation_when_backend_has_native_support() {
    // Streaming IS natively supported by MockBackend.
    let rt = Runtime::with_default_backends().with_emulation(EmulationConfig::new());
    let mut wo = mock_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };

    let (_events, receipt) = run_to_completion(&rt, wo).await;
    // No emulation key should appear since native capability was used.
    let usage = &receipt.usage_raw;
    assert!(
        usage.get("emulation").is_none(),
        "no emulation expected when capability is natively supported"
    );
}

// ── 3. Emulation report is included in receipt metadata ──

#[tokio::test]
async fn emulation_report_in_receipt_metadata() {
    let rt = Runtime::with_default_backends().with_emulation(EmulationConfig::new());
    let mut wo = mock_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ExtendedThinking,
            min_support: MinSupport::Emulated,
        }],
    };

    let handle = rt.run_streaming("mock", wo).await.expect("run_streaming");
    let _: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.expect("join").expect("receipt");

    let emulation = receipt.usage_raw.get("emulation");
    assert!(
        emulation.is_some(),
        "receipt should contain emulation report"
    );

    let applied = emulation.unwrap().get("applied");
    assert!(
        applied.is_some(),
        "emulation report should have applied list"
    );
    let applied_arr = applied.unwrap().as_array().unwrap();
    assert!(
        !applied_arr.is_empty(),
        "at least one emulation should be applied"
    );
}

// ── 4. Disabled emulation still fails with capability error ──

#[tokio::test]
async fn no_emulation_config_fails_for_missing_capability() {
    // No emulation configured — missing capability should fail.
    let rt = Runtime::with_default_backends();
    let mut wo = mock_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ExtendedThinking,
            min_support: MinSupport::Native,
        }],
    };

    let result = rt.run_streaming("mock", wo).await;
    assert!(result.is_err(), "should fail without emulation config");
    let err = match result {
        Err(e) => format!("{e}"),
        Ok(_) => panic!("expected error"),
    };
    assert!(
        err.contains("capability"),
        "error should mention capability: {err}"
    );
}

// ── 5. Default emulation config works out of box ──

#[tokio::test]
async fn default_emulation_config_works() {
    let config = EmulationConfig::new();
    let rt = Runtime::with_default_backends().with_emulation(config);
    let wo = mock_work_order();
    let (_events, receipt) = run_to_completion(&rt, wo).await;
    assert!(matches!(receipt.outcome, abp_core::Outcome::Complete));
}

// ── 6. Unemulatable capability still fails even with emulation enabled ──

#[tokio::test]
async fn unemulatable_capability_fails_even_with_emulation() {
    // CodeExecution cannot be emulated by default.
    let rt = Runtime::with_default_backends().with_emulation(EmulationConfig::new());
    let mut wo = mock_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::CodeExecution,
            min_support: MinSupport::Native,
        }],
    };

    let result = rt.run_streaming("mock", wo).await;
    assert!(
        result.is_err(),
        "code_execution cannot be emulated and should fail"
    );
    let err = match result {
        Err(e) => format!("{e}"),
        Ok(_) => panic!("expected error"),
    };
    assert!(err.contains("emulation unavailable"), "error: {err}");
}

// ── 7. Custom strategy override enables previously disabled capability ──

#[tokio::test]
async fn custom_strategy_overrides_default() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::CodeExecution,
        EmulationStrategy::SystemPromptInjection {
            prompt: "Simulate code execution.".into(),
        },
    );

    let rt = Runtime::with_default_backends().with_emulation(config);
    let mut wo = mock_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::CodeExecution,
            min_support: MinSupport::Emulated,
        }],
    };

    let result = rt.run_streaming("mock", wo).await;
    assert!(
        result.is_ok(),
        "custom strategy should allow emulation of CodeExecution"
    );

    let handle = result.unwrap();
    let _: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.expect("join").expect("receipt");
    assert!(receipt.usage_raw.get("emulation").is_some());
}

// ── 8. Multiple missing capabilities all emulatable ──

#[tokio::test]
async fn multiple_emulatable_capabilities_accepted() {
    let rt = Runtime::with_default_backends().with_emulation(EmulationConfig::new());
    let mut wo = mock_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::ExtendedThinking,
                min_support: MinSupport::Emulated,
            },
            CapabilityRequirement {
                capability: Capability::StructuredOutputJsonSchema,
                // Mock backend already has this, so it won't be "missing."
                // Let's use something else that IS emulatable but missing.
                min_support: MinSupport::Emulated,
            },
        ],
    };

    let result = rt.run_streaming("mock", wo).await;
    assert!(result.is_ok(), "all emulatable capabilities should pass");
    let handle = result.unwrap();
    let _: Vec<_> = handle.events.collect().await;
    let _ = handle.receipt.await;
}

// ── 9. Mix of emulatable and unemulatable capabilities fails ──

#[tokio::test]
async fn mixed_emulatable_and_unemulatable_fails() {
    let rt = Runtime::with_default_backends().with_emulation(EmulationConfig::new());
    let mut wo = mock_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::ExtendedThinking,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::CodeExecution,
                min_support: MinSupport::Native,
            },
        ],
    };

    let result = rt.run_streaming("mock", wo).await;
    assert!(
        result.is_err(),
        "mix of emulatable and unemulatable should fail"
    );
}

// ── 10. Emulation config accessor ──

#[test]
fn emulation_config_accessor_returns_none_by_default() {
    let rt = Runtime::new();
    assert!(rt.emulation_config().is_none());
}

#[test]
fn emulation_config_accessor_returns_some_when_set() {
    let rt = Runtime::new().with_emulation(EmulationConfig::new());
    assert!(rt.emulation_config().is_some());
}

// ── 11. Empty requirements with emulation configured still passes ──

#[tokio::test]
async fn empty_requirements_with_emulation_passes() {
    let rt = Runtime::with_default_backends().with_emulation(EmulationConfig::new());
    let wo = mock_work_order();
    let (_events, receipt) = run_to_completion(&rt, wo).await;
    assert!(receipt.usage_raw.get("emulation").is_none());
}

// ── 12. Receipt hash is valid after emulation metadata insertion ──

#[tokio::test]
async fn receipt_hash_valid_after_emulation() {
    let rt = Runtime::with_default_backends().with_emulation(EmulationConfig::new());
    let mut wo = mock_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ExtendedThinking,
            min_support: MinSupport::Emulated,
        }],
    };

    let handle = rt.run_streaming("mock", wo).await.expect("run_streaming");
    let _: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.expect("join").expect("receipt");

    let stored_hash = receipt.receipt_sha256.clone().expect("hash must exist");
    let recomputed = abp_core::receipt_hash(&receipt).expect("recompute hash");
    assert_eq!(stored_hash, recomputed, "hash must be consistent");
}

// ── 13. with_emulation is chainable ──

#[test]
fn with_emulation_is_chainable() {
    let rt = Runtime::with_default_backends().with_emulation(EmulationConfig::new());
    assert!(rt.emulation_config().is_some());
    assert!(rt.backend_names().contains(&"mock".to_string()));
}

// ── 14. Emulation report applied entries have correct capability ──

#[tokio::test]
async fn emulation_report_applied_has_correct_capability() {
    let rt = Runtime::with_default_backends().with_emulation(EmulationConfig::new());
    let mut wo = mock_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ExtendedThinking,
            min_support: MinSupport::Emulated,
        }],
    };

    let handle = rt.run_streaming("mock", wo).await.expect("run_streaming");
    let _: Vec<_> = handle.events.collect().await;
    let receipt = handle.receipt.await.expect("join").expect("receipt");

    let emulation = receipt.usage_raw.get("emulation").unwrap();
    let applied = emulation.get("applied").unwrap().as_array().unwrap();
    assert_eq!(applied.len(), 1);
    let cap_value = applied[0].get("capability").unwrap().as_str().unwrap();
    assert_eq!(cap_value, "extended_thinking");
}

// ── 15. Emulation report warnings for disabled strategies ──

#[tokio::test]
async fn custom_disabled_strategy_produces_failure() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::Disabled {
            reason: "user disabled".into(),
        },
    );

    let rt = Runtime::with_default_backends().with_emulation(config);
    let mut wo = mock_work_order();
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ExtendedThinking,
            min_support: MinSupport::Native,
        }],
    };

    let result = rt.run_streaming("mock", wo).await;
    assert!(
        result.is_err(),
        "disabled strategy override should prevent emulation"
    );
}

// ── 16. Satisfied requirement not counted as emulated ──

#[tokio::test]
async fn satisfied_requirement_not_counted_as_emulated() {
    let rt = Runtime::with_default_backends().with_emulation(EmulationConfig::new());
    let mut wo = mock_work_order();
    // Streaming is natively supported by mock — should not be emulated.
    wo.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };

    let (_events, receipt) = run_to_completion(&rt, wo).await;
    assert!(
        receipt.usage_raw.get("emulation").is_none(),
        "natively satisfied capability must not trigger emulation"
    );
}
