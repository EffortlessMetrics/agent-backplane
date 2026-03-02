// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for capability negotiation integration in the runtime pipeline.

use abp_core::{
    Capability, CapabilityRequirement, CapabilityRequirements, ExecutionLane, MinSupport,
    PolicyProfile, WorkOrder, WorkspaceMode, WorkspaceSpec,
};
use abp_runtime::Runtime;
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
        .get("emulatable")
        .and_then(|v| v.as_array())
        .expect("emulatable array");
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
