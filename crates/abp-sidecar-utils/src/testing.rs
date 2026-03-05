// SPDX-License-Identifier: MIT OR Apache-2.0
//! Testing helpers — pre-built mock [`Envelope`] values and work order
//! generators for use in tests and examples.

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, Outcome, Receipt,
    ReceiptBuilder, WorkOrderBuilder,
};
use abp_protocol::{Envelope, JsonlCodec};
use chrono::Utc;

/// Create a mock `Hello` envelope for the given backend name.
///
/// Uses the current [`CONTRACT_VERSION`](abp_core::CONTRACT_VERSION) and an
/// empty capability manifest.
///
/// # Examples
///
/// ```
/// let hello = abp_sidecar_utils::testing::mock_hello("test-backend");
/// assert!(matches!(hello, abp_protocol::Envelope::Hello { .. }));
/// ```
#[must_use]
pub fn mock_hello(backend: &str) -> Envelope {
    Envelope::hello(
        BackendIdentity {
            id: backend.into(),
            backend_version: Some("0.1.0".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    )
}

/// Create a mock `Event` envelope with an [`AssistantMessage`](AgentEventKind::AssistantMessage).
///
/// # Examples
///
/// ```
/// let event = abp_sidecar_utils::testing::mock_event("run-1", "hello world");
/// assert!(matches!(event, abp_protocol::Envelope::Event { .. }));
/// ```
#[must_use]
pub fn mock_event(ref_id: &str, text: &str) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: text.into() },
            ext: None,
        },
    }
}

/// Create a mock `Final` envelope with a completed receipt.
///
/// # Examples
///
/// ```
/// let final_env = abp_sidecar_utils::testing::mock_final("run-1");
/// assert!(matches!(final_env, abp_protocol::Envelope::Final { .. }));
/// ```
#[must_use]
pub fn mock_final(ref_id: &str) -> Envelope {
    let receipt: Receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    Envelope::Final {
        ref_id: ref_id.into(),
        receipt,
    }
}

/// Create a mock `Fatal` envelope with the given error message.
///
/// # Examples
///
/// ```
/// let fatal = abp_sidecar_utils::testing::mock_fatal("run-1", "out of memory");
/// assert!(matches!(fatal, abp_protocol::Envelope::Fatal { error, .. } if error == "out of memory"));
/// ```
#[must_use]
pub fn mock_fatal(ref_id: &str, error: &str) -> Envelope {
    Envelope::Fatal {
        ref_id: Some(ref_id.into()),
        error: error.into(),
        error_code: None,
    }
}

/// Create a minimal `Run` envelope as a JSONL string for the given task.
///
/// This is useful for feeding test input to sidecar processes.
///
/// # Examples
///
/// ```
/// let line = abp_sidecar_utils::testing::mock_work_order("fix the bug");
/// assert!(line.contains("\"t\":\"run\""));
/// assert!(line.contains("fix the bug"));
/// ```
#[must_use]
pub fn mock_work_order(task: &str) -> String {
    let wo = WorkOrderBuilder::new(task).build();
    let envelope = Envelope::Run {
        id: wo.id.to_string(),
        work_order: wo,
    };
    JsonlCodec::encode(&envelope).expect("run envelope serialization should not fail")
}
