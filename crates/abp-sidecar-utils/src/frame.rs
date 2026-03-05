// SPDX-License-Identifier: MIT OR Apache-2.0
//! Frame helpers — convenient encode/decode functions for [`Envelope`] messages.
//!
//! These functions wrap [`abp_protocol::JsonlCodec`] and the [`Envelope`]
//! constructors to provide one-liner helpers for the most common sidecar
//! protocol operations.

use abp_core::{
    AgentEvent, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest, Receipt,
    SupportLevel,
};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};

/// Serialize an [`Envelope`] to a newline-terminated JSONL line.
///
/// # Errors
///
/// Returns [`ProtocolError::Json`] if the envelope cannot be serialized.
pub fn encode_envelope(envelope: &Envelope) -> Result<String, ProtocolError> {
    JsonlCodec::encode(envelope)
}

/// Parse a single JSONL line into an [`Envelope`].
///
/// The input may or may not have a trailing newline; it is trimmed before
/// parsing.
///
/// # Errors
///
/// Returns [`ProtocolError::Json`] if the line is not a valid envelope.
pub fn decode_envelope(line: &str) -> Result<Envelope, ProtocolError> {
    JsonlCodec::decode(line.trim())
}

/// Build and encode a `Hello` envelope.
///
/// `capabilities` is a slice of capability name strings that will be parsed
/// into [`Capability`] values using serde. Unrecognised names are silently
/// skipped.
///
/// # Examples
///
/// ```
/// let line = abp_sidecar_utils::frame::encode_hello("my-backend", "1.0", &["streaming"]);
/// assert!(line.contains("\"t\":\"hello\""));
/// ```
pub fn encode_hello(backend_name: &str, version: &str, capabilities: &[&str]) -> String {
    let backend = BackendIdentity {
        id: backend_name.into(),
        backend_version: Some(version.into()),
        adapter_version: None,
    };

    let mut manifest = CapabilityManifest::new();
    for cap_str in capabilities {
        if let Ok(cap) =
            serde_json::from_value::<Capability>(serde_json::Value::String(cap_str.to_string()))
        {
            manifest.insert(cap, SupportLevel::Native);
        }
    }

    let envelope = Envelope::hello(backend, manifest);
    JsonlCodec::encode(&envelope).expect("hello envelope serialization should not fail")
}

/// Build and encode an `Event` envelope.
///
/// # Examples
///
/// ```
/// use abp_core::{AgentEvent, AgentEventKind};
/// use chrono::Utc;
///
/// let event = AgentEvent {
///     ts: Utc::now(),
///     kind: AgentEventKind::AssistantMessage { text: "hi".into() },
///     ext: None,
/// };
/// let line = abp_sidecar_utils::frame::encode_event("run-1", &event);
/// assert!(line.contains("\"t\":\"event\""));
/// ```
pub fn encode_event(ref_id: &str, event: &AgentEvent) -> String {
    let envelope = Envelope::Event {
        ref_id: ref_id.into(),
        event: event.clone(),
    };
    JsonlCodec::encode(&envelope).expect("event envelope serialization should not fail")
}

/// Build and encode a `Final` envelope.
///
/// # Examples
///
/// ```
/// let receipt = abp_core::ReceiptBuilder::new("mock")
///     .outcome(abp_core::Outcome::Complete)
///     .build();
/// let line = abp_sidecar_utils::frame::encode_final("run-1", &receipt);
/// assert!(line.contains("\"t\":\"final\""));
/// ```
pub fn encode_final(ref_id: &str, receipt: &Receipt) -> String {
    let envelope = Envelope::Final {
        ref_id: ref_id.into(),
        receipt: receipt.clone(),
    };
    JsonlCodec::encode(&envelope).expect("final envelope serialization should not fail")
}

/// Build and encode a `Fatal` envelope.
///
/// # Examples
///
/// ```
/// let line = abp_sidecar_utils::frame::encode_fatal("run-1", "out of memory");
/// assert!(line.contains("\"t\":\"fatal\""));
/// assert!(line.contains("out of memory"));
/// ```
pub fn encode_fatal(ref_id: &str, error: &str) -> String {
    let envelope = Envelope::Fatal {
        ref_id: Some(ref_id.into()),
        error: error.into(),
        error_code: None,
    };
    JsonlCodec::encode(&envelope).expect("fatal envelope serialization should not fail")
}

/// Convenience: build a [`BackendIdentity`] from a name and version.
#[must_use]
pub fn backend_identity(name: &str, version: &str) -> BackendIdentity {
    BackendIdentity {
        id: name.into(),
        backend_version: Some(version.into()),
        adapter_version: None,
    }
}

/// Return the current [`CONTRACT_VERSION`] string.
#[must_use]
pub fn contract_version() -> &'static str {
    CONTRACT_VERSION
}
