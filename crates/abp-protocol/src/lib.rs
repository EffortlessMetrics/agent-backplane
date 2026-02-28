// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
//! abp-protocol
#![deny(unsafe_code)]
//!
//! Wire format for talking to sidecars and daemons.
//! Current transport: JSONL over stdio.

pub mod codec;
pub mod version;

use std::io::{BufRead, Write};

use abp_core::{
    AgentEvent, BackendIdentity, CONTRACT_VERSION, CapabilityManifest, ExecutionMode, Receipt,
    WorkOrder,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// JSONL message envelope.
///
/// The protocol is intentionally simple:
/// - sidecar announces itself via `hello`
/// - control plane sends `run`
/// - sidecar streams `event`
/// - sidecar concludes with `final` (receipt)
///
/// # Examples
///
/// ```
/// use abp_core::{BackendIdentity, CapabilityManifest};
/// use abp_protocol::{Envelope, JsonlCodec};
///
/// let hello = Envelope::hello(
///     BackendIdentity {
///         id: "my-sidecar".into(),
///         backend_version: Some("1.0.0".into()),
///         adapter_version: None,
///     },
///     CapabilityManifest::new(),
/// );
///
/// // Serialize to a newline-terminated JSON string.
/// let line = JsonlCodec::encode(&hello).unwrap();
/// assert!(line.ends_with('\n'));
/// assert!(line.contains("\"t\":\"hello\""));
///
/// // Round-trip back to an Envelope.
/// let decoded = JsonlCodec::decode(line.trim()).unwrap();
/// assert!(matches!(decoded, Envelope::Hello { .. }));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Envelope {
    Hello {
        contract_version: String,
        backend: BackendIdentity,
        capabilities: CapabilityManifest,
        /// Execution mode this sidecar will use. Defaults to "mapped" if absent.
        #[serde(default)]
        mode: ExecutionMode,
    },

    Run {
        id: String,
        work_order: WorkOrder,
    },

    Event {
        ref_id: String,
        event: AgentEvent,
    },

    Final {
        ref_id: String,
        receipt: Receipt,
    },

    Fatal {
        ref_id: Option<String>,
        error: String,
    },
}

impl Envelope {
    /// Create a `Hello` envelope with the default execution mode (Mapped).
    #[must_use]
    pub fn hello(backend: BackendIdentity, capabilities: CapabilityManifest) -> Self {
        Self::hello_with_mode(backend, capabilities, ExecutionMode::default())
    }

    /// Create a `Hello` envelope with an explicit [`ExecutionMode`].
    #[must_use]
    pub fn hello_with_mode(
        backend: BackendIdentity,
        capabilities: CapabilityManifest,
        mode: ExecutionMode,
    ) -> Self {
        Self::Hello {
            contract_version: CONTRACT_VERSION.to_string(),
            backend,
            capabilities,
            mode,
        }
    }
}

/// Errors arising from JSONL encoding/decoding or protocol-level violations.
#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("protocol violation: {0}")]
    Violation(String),

    #[error("unexpected message: expected {expected}, got {got}")]
    UnexpectedMessage { expected: String, got: String },
}

/// Stateless codec for encoding/decoding [`Envelope`] messages as newline-delimited JSON.
#[derive(Debug, Clone, Copy)]
pub struct JsonlCodec;

impl JsonlCodec {
    /// Serialize an [`Envelope`] to a newline-terminated JSON string.
    ///
    /// # Examples
    ///
    /// ```
    /// # use abp_core::{BackendIdentity, CapabilityManifest};
    /// # use abp_protocol::{Envelope, JsonlCodec};
    /// let envelope = Envelope::Fatal {
    ///     ref_id: Some("run-123".into()),
    ///     error: "out of memory".into(),
    /// };
    /// let json = JsonlCodec::encode(&envelope).unwrap();
    /// assert!(json.ends_with('\n'));
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError::Json`] if the envelope cannot be serialized.
    pub fn encode(msg: &Envelope) -> Result<String, ProtocolError> {
        let mut s = serde_json::to_string(msg)?;
        s.push('\n');
        Ok(s)
    }

    /// Deserialize a single JSON line into an [`Envelope`].
    ///
    /// # Examples
    ///
    /// ```
    /// use abp_protocol::{Envelope, JsonlCodec};
    ///
    /// let line = r#"{"t":"fatal","ref_id":null,"error":"boom"}"#;
    /// let envelope = JsonlCodec::decode(line).unwrap();
    /// assert!(matches!(envelope, Envelope::Fatal { error, .. } if error == "boom"));
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError::Json`] if the line is not valid JSON or does
    /// not match any [`Envelope`] variant.
    pub fn decode(line: &str) -> Result<Envelope, ProtocolError> {
        Ok(serde_json::from_str::<Envelope>(line)?)
    }

    /// Return a lazy iterator that reads JSONL lines from `reader`, skipping
    /// blank lines, and deserializing each into an [`Envelope`].
    ///
    /// The iterator yields one `Result<Envelope>` per non-blank line.
    ///
    /// # Examples
    ///
    /// ```
    /// use abp_protocol::{Envelope, JsonlCodec};
    /// use std::io::BufReader;
    ///
    /// let input = r#"{"t":"fatal","ref_id":null,"error":"boom"}
    /// {"t":"fatal","ref_id":null,"error":"bang"}
    /// "#;
    /// let reader = BufReader::new(input.as_bytes());
    /// let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
    ///     .collect::<Result<Vec<_>, _>>()
    ///     .unwrap();
    /// assert_eq!(envelopes.len(), 2);
    /// ```
    pub fn decode_stream(
        reader: impl BufRead,
    ) -> impl Iterator<Item = Result<Envelope, ProtocolError>> {
        reader.lines().filter_map(|line_result| match line_result {
            Err(e) => Some(Err(ProtocolError::Io(e))),
            Ok(line) => {
                if line.trim().is_empty() {
                    None
                } else {
                    Some(Self::decode(line.trim()))
                }
            }
        })
    }

    /// Write a single [`Envelope`] as a newline-terminated JSON line.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError`] on serialization or I/O failure.
    pub fn encode_to_writer(
        writer: &mut impl Write,
        envelope: &Envelope,
    ) -> Result<(), ProtocolError> {
        let line = Self::encode(envelope)?;
        writer.write_all(line.as_bytes())?;
        Ok(())
    }

    /// Write multiple [`Envelope`]s as consecutive JSONL lines.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError`] on serialization or I/O failure.
    pub fn encode_many_to_writer(
        writer: &mut impl Write,
        envelopes: &[Envelope],
    ) -> Result<(), ProtocolError> {
        for env in envelopes {
            Self::encode_to_writer(writer, env)?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Version negotiation helpers
// ---------------------------------------------------------------------------

/// Parse a version string of the form `"abp/vMAJOR.MINOR"` into `(MAJOR, MINOR)`.
///
/// Returns `None` if the string does not match the expected format.
#[must_use]
pub fn parse_version(version: &str) -> Option<(u32, u32)> {
    let rest = version.strip_prefix("abp/v")?;
    let (major_str, minor_str) = rest.split_once('.')?;
    let major = major_str.parse::<u32>().ok()?;
    let minor = minor_str.parse::<u32>().ok()?;
    Some((major, minor))
}

/// Two versions are compatible when they share the same major component.
///
/// For example `"abp/v0.1"` and `"abp/v0.2"` are compatible, but
/// `"abp/v1.0"` and `"abp/v0.1"` are not.  Returns `false` if either
/// string cannot be parsed.
#[must_use]
pub fn is_compatible_version(their_version: &str, our_version: &str) -> bool {
    match (parse_version(their_version), parse_version(our_version)) {
        (Some((their_major, _)), Some((our_major, _))) => their_major == our_major,
        _ => false,
    }
}

/// Re-export of the value-based [`sidecar_kit::Frame`] for raw protocol work.
pub use sidecar_kit::Frame as RawFrame;
/// Re-export of the value-based [`sidecar_kit::JsonlCodec`] for raw JSONL encoding.
pub use sidecar_kit::JsonlCodec as RawCodec;
