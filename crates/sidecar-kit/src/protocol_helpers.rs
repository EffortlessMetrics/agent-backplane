// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code, unused_imports)]
//! Protocol helpers for reading and writing JSONL frames.
//!
//! These functions handle serialization, newline termination, and frame
//! construction for the ABP sidecar protocol. They work with the
//! value-based [`Frame`](crate::Frame) type and accept typed `abp-core`
//! values, converting them to JSON for wire transmission.
//!
//! # Example
//! ```
//! use sidecar_kit::protocol_helpers::{send_hello, send_event, send_final, send_fatal};
//! use sidecar_kit::events::text_event;
//! use sidecar_kit::receipt_builder::TypedReceiptBuilder;
//! use abp_core::CapabilityManifest;
//!
//! let mut out = Vec::new();
//! send_hello(&mut out, "my-sidecar", &CapabilityManifest::new()).unwrap();
//! send_event(&mut out, "run-1", &text_event("hello")).unwrap();
//!
//! let receipt = TypedReceiptBuilder::new("my-sidecar").build();
//! send_final(&mut out, "run-1", &receipt).unwrap();
//!
//! let output = String::from_utf8(out).unwrap();
//! assert!(output.contains("\"t\":\"hello\""));
//! assert!(output.contains("\"t\":\"event\""));
//! assert!(output.contains("\"t\":\"final\""));
//! ```

use std::io::{BufRead, Write};

use serde_json::Value;

use abp_core::{AgentEvent, CapabilityManifest, Receipt, WorkOrder};

use crate::codec::JsonlCodec;
use crate::error::SidecarError;
use crate::frame::Frame;

/// Write a `hello` envelope to the given writer.
///
/// Constructs a hello frame with the ABP contract version and the
/// provided backend identifier and capabilities.
///
/// # Errors
///
/// Returns [`SidecarError`] on serialization or I/O failure.
pub fn send_hello(
    writer: &mut impl Write,
    backend_id: &str,
    capabilities: &CapabilityManifest,
) -> Result<(), SidecarError> {
    let caps_value =
        serde_json::to_value(capabilities).map_err(SidecarError::Serialize)?;
    let frame = Frame::Hello {
        contract_version: abp_core::CONTRACT_VERSION.to_string(),
        backend: serde_json::json!({ "id": backend_id }),
        capabilities: caps_value,
        mode: Value::Null,
    };
    write_frame(writer, &frame)
}

/// Read a `run` envelope from the given reader, returning the [`WorkOrder`].
///
/// Skips blank lines and expects the next non-blank line to be a `run` frame.
///
/// # Errors
///
/// Returns [`SidecarError`] if the frame is not a `run` or on I/O/parse failure.
pub fn read_run(reader: &mut impl BufRead) -> Result<WorkOrder, SidecarError> {
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).map_err(SidecarError::Stdout)?;
        if n == 0 {
            return Err(SidecarError::Protocol("unexpected EOF waiting for run".into()));
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let frame: Frame = JsonlCodec::decode(trimmed)?;
        match frame {
            Frame::Run { id: _, work_order } => {
                let wo: WorkOrder =
                    serde_json::from_value(work_order).map_err(SidecarError::Deserialize)?;
                return Ok(wo);
            }
            _ => {
                return Err(SidecarError::Protocol(format!(
                    "expected run frame, got: {frame:?}"
                )));
            }
        }
    }
}

/// Write an `event` envelope to the given writer.
///
/// Serializes the [`AgentEvent`] to JSON and wraps it in a `Frame::Event`.
///
/// # Errors
///
/// Returns [`SidecarError`] on serialization or I/O failure.
pub fn send_event(
    writer: &mut impl Write,
    ref_id: &str,
    event: &AgentEvent,
) -> Result<(), SidecarError> {
    let event_value = serde_json::to_value(event).map_err(SidecarError::Serialize)?;
    let frame = Frame::Event {
        ref_id: ref_id.to_string(),
        event: event_value,
    };
    write_frame(writer, &frame)
}

/// Write a `final` envelope to the given writer.
///
/// Serializes the [`Receipt`] to JSON and wraps it in a `Frame::Final`.
///
/// # Errors
///
/// Returns [`SidecarError`] on serialization or I/O failure.
pub fn send_final(
    writer: &mut impl Write,
    ref_id: &str,
    receipt: &Receipt,
) -> Result<(), SidecarError> {
    let receipt_value = serde_json::to_value(receipt).map_err(SidecarError::Serialize)?;
    let frame = Frame::Final {
        ref_id: ref_id.to_string(),
        receipt: receipt_value,
    };
    write_frame(writer, &frame)
}

/// Write a `fatal` envelope to the given writer.
///
/// # Errors
///
/// Returns [`SidecarError`] on serialization or I/O failure.
pub fn send_fatal(
    writer: &mut impl Write,
    ref_id: Option<&str>,
    _error_code: &str,
    message: &str,
) -> Result<(), SidecarError> {
    let frame = Frame::Fatal {
        ref_id: ref_id.map(str::to_string),
        error: format!("[{_error_code}] {message}"),
    };
    write_frame(writer, &frame)
}

// ── Internal helper ─────────────────────────────────────────────────

fn write_frame(writer: &mut impl Write, frame: &Frame) -> Result<(), SidecarError> {
    let line = JsonlCodec::encode(frame)?;
    writer.write_all(line.as_bytes()).map_err(SidecarError::Stdin)?;
    writer.flush().map_err(SidecarError::Stdin)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::CapabilityManifest;
    use crate::events::text_event;
    use crate::receipt_builder::TypedReceiptBuilder;

    #[test]
    fn send_hello_produces_valid_jsonl() {
        let mut buf = Vec::new();
        send_hello(&mut buf, "test-sidecar", &CapabilityManifest::new()).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.ends_with('\n'));
        assert!(output.contains("\"t\":\"hello\""));
        assert!(output.contains("\"contract_version\":\"abp/v0.1\""));
        assert!(output.contains("\"test-sidecar\""));
    }

    #[test]
    fn send_event_produces_valid_jsonl() {
        let mut buf = Vec::new();
        let event = text_event("hello world");
        send_event(&mut buf, "run-1", &event).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.ends_with('\n'));
        assert!(output.contains("\"t\":\"event\""));
        assert!(output.contains("\"ref_id\":\"run-1\""));
        assert!(output.contains("hello world"));
    }

    #[test]
    fn send_final_produces_valid_jsonl() {
        let mut buf = Vec::new();
        let receipt = TypedReceiptBuilder::new("test").build();
        send_final(&mut buf, "run-1", &receipt).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.ends_with('\n'));
        assert!(output.contains("\"t\":\"final\""));
        assert!(output.contains("\"ref_id\":\"run-1\""));
    }

    #[test]
    fn send_fatal_produces_valid_jsonl() {
        let mut buf = Vec::new();
        send_fatal(&mut buf, Some("run-1"), "E500", "internal error").unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.ends_with('\n'));
        assert!(output.contains("\"t\":\"fatal\""));
        assert!(output.contains("internal error"));
        assert!(output.contains("E500"));
    }

    #[test]
    fn send_fatal_without_ref_id() {
        let mut buf = Vec::new();
        send_fatal(&mut buf, None, "E999", "startup failure").unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("\"ref_id\":null"));
        assert!(output.contains("startup failure"));
    }

    #[test]
    fn read_run_parses_work_order() {
        // Build a run frame with a minimal work order
        let wo = abp_core::WorkOrder {
            id: uuid::Uuid::nil(),
            task: "do the thing".into(),
            lane: abp_core::ExecutionLane::PatchFirst,
            workspace: abp_core::WorkspaceSpec {
                root: "/tmp".into(),
                mode: abp_core::WorkspaceMode::Staged,
                include: vec![],
                exclude: vec![],
            },
            context: abp_core::ContextPacket {
                files: vec![],
                snippets: vec![],
            },
            policy: abp_core::PolicyProfile {
                allowed_tools: vec!["*".into()],
                disallowed_tools: vec![],
                deny_read: vec![],
                deny_write: vec![],
                allow_network: vec![],
                deny_network: vec![],
                require_approval_for: vec![],
            },
            requirements: abp_core::CapabilityRequirements {
                required: vec![],
            },
            config: abp_core::RuntimeConfig {
                model: Some("test-model".into()),
                vendor: Default::default(),
                env: Default::default(),
                max_budget_usd: None,
                max_turns: Some(10),
            },
        };
        let wo_value = serde_json::to_value(&wo).unwrap();
        let frame = Frame::Run {
            id: "run-1".to_string(),
            work_order: wo_value,
        };
        let line = JsonlCodec::encode(&frame).unwrap();

        let mut reader = std::io::BufReader::new(line.as_bytes());
        let parsed = read_run(&mut reader).unwrap();
        assert_eq!(parsed.task, "do the thing");
    }

    #[test]
    fn read_run_skips_blank_lines() {
        let wo = abp_core::WorkOrder {
            id: uuid::Uuid::nil(),
            task: "test".into(),
            lane: abp_core::ExecutionLane::PatchFirst,
            workspace: abp_core::WorkspaceSpec {
                root: "/tmp".into(),
                mode: abp_core::WorkspaceMode::Staged,
                include: vec![],
                exclude: vec![],
            },
            context: abp_core::ContextPacket {
                files: vec![],
                snippets: vec![],
            },
            policy: abp_core::PolicyProfile {
                allowed_tools: vec!["*".into()],
                disallowed_tools: vec![],
                deny_read: vec![],
                deny_write: vec![],
                allow_network: vec![],
                deny_network: vec![],
                require_approval_for: vec![],
            },
            requirements: abp_core::CapabilityRequirements {
                required: vec![],
            },
            config: abp_core::RuntimeConfig {
                model: Some("m".into()),
                vendor: Default::default(),
                env: Default::default(),
                max_budget_usd: None,
                max_turns: None,
            },
        };
        let wo_value = serde_json::to_value(&wo).unwrap();
        let frame = Frame::Run {
            id: "run-1".to_string(),
            work_order: wo_value,
        };
        let line = JsonlCodec::encode(&frame).unwrap();
        let input = format!("\n\n{line}");

        let mut reader = std::io::BufReader::new(input.as_bytes());
        let parsed = read_run(&mut reader).unwrap();
        assert_eq!(parsed.task, "test");
    }

    #[test]
    fn read_run_eof_returns_error() {
        let mut reader = std::io::BufReader::new("".as_bytes());
        let result = read_run(&mut reader);
        assert!(result.is_err());
    }

    #[test]
    fn full_protocol_sequence() {
        let mut buf = Vec::new();

        // 1. hello
        send_hello(&mut buf, "my-sidecar", &CapabilityManifest::new()).unwrap();

        // 2. events
        let e1 = crate::events::run_started_event("starting");
        send_event(&mut buf, "run-1", &e1).unwrap();

        let e2 = text_event("output");
        send_event(&mut buf, "run-1", &e2).unwrap();

        // 3. final
        let receipt = TypedReceiptBuilder::new("my-sidecar")
            .add_event(e1)
            .add_event(e2)
            .build();
        send_final(&mut buf, "run-1", &receipt).unwrap();

        // Verify all lines parse as valid frames
        let output = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 4);

        for line in &lines {
            let _frame: Frame = serde_json::from_str(line).expect("valid frame");
        }
    }
}
