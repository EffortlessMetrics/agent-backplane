// SPDX-License-Identifier: MIT OR Apache-2.0
//! Sidecar harness for Rust-based sidecar implementations.
//!
//! The [`SidecarHarness`] manages the JSONL protocol lifecycle over
//! stdin/stdout so that sidecar authors only need to implement the
//! [`SidecarHandler`] trait.
//!
//! # Example
//!
//! ```no_run
//! use serde_json::Value;
//! use sidecar_kit::harness::{SidecarHandler, SidecarHarness, HandlerContext};
//! use sidecar_kit::capabilities::CapabilitySet;
//!
//! struct MySidecar;
//!
//! impl SidecarHandler for MySidecar {
//!     fn backend_id(&self) -> &str { "my-sidecar" }
//!
//!     fn capabilities(&self) -> CapabilitySet {
//!         CapabilitySet::new().native("streaming")
//!     }
//!
//!     fn handle_run(&self, ctx: HandlerContext) -> Result<Value, String> {
//!         ctx.emit_event(sidecar_kit::builders::event_text_message("Hello!"));
//!         Ok(sidecar_kit::ReceiptBuilder::new(&ctx.run_id, "my-sidecar").build())
//!     }
//! }
//! ```

use std::io::{self, BufRead, Write};
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::capabilities::CapabilitySet;
use crate::codec::JsonlCodec;
use crate::frame::Frame;
use crate::work_order::WorkOrderView;

/// Context provided to [`SidecarHandler::handle_run`].
///
/// Holds the run identifier and work order, and provides a method to
/// emit streaming events back to the control plane.
pub struct HandlerContext {
    /// The unique run identifier from the `Run` frame.
    pub run_id: String,
    /// The raw work order value.
    pub work_order: Value,
    event_buf: Arc<Mutex<Vec<u8>>>,
}

impl HandlerContext {
    /// A convenience view into the work order.
    #[must_use]
    pub fn work_order_view(&self) -> WorkOrderView<'_> {
        WorkOrderView::new(&self.work_order)
    }

    /// Emit a streaming event back to the control plane.
    ///
    /// The event value should match an ABP `AgentEvent` shape (use the
    /// helpers in [`crate::builders`]).
    pub fn emit_event(&self, event: Value) {
        let frame = Frame::Event {
            ref_id: self.run_id.clone(),
            event,
        };
        if let Ok(line) = JsonlCodec::encode(&frame) {
            let mut buf = self.event_buf.lock().unwrap();
            let _ = buf.write_all(line.as_bytes());
        }
    }
}

/// Trait that sidecar authors implement to handle work orders.
pub trait SidecarHandler {
    /// The backend identifier included in the `hello` handshake.
    fn backend_id(&self) -> &str;

    /// Capabilities advertised in the `hello` handshake.
    ///
    /// Default: empty set.
    fn capabilities(&self) -> CapabilitySet {
        CapabilitySet::new()
    }

    /// Handle a single run.
    ///
    /// Use [`HandlerContext::emit_event`] to stream events.
    /// Return `Ok(receipt_value)` on success or `Err(message)` on fatal error.
    fn handle_run(&self, ctx: HandlerContext) -> Result<Value, String>;
}

/// Manages the JSONL protocol lifecycle for a Rust-based sidecar.
///
/// Reads from `reader` (typically stdin), writes to `writer` (typically stdout),
/// and delegates run handling to a [`SidecarHandler`].
pub struct SidecarHarness<H: SidecarHandler> {
    handler: H,
}

impl<H: SidecarHandler> SidecarHarness<H> {
    /// Create a new harness wrapping the given handler.
    pub fn new(handler: H) -> Self {
        Self { handler }
    }

    /// Run the protocol loop over the given reader/writer pair.
    ///
    /// Sends `hello`, waits for `run`, delegates to the handler, and
    /// sends `final` or `fatal` as appropriate.
    ///
    /// Returns `Ok(())` on clean shutdown (EOF or successful run).
    pub fn run<R: BufRead, W: Write>(
        &self,
        mut reader: R,
        mut writer: W,
    ) -> Result<(), HarnessError> {
        // 1. Send hello
        let caps = self.handler.capabilities();
        let hello = hello_frame_with_caps(self.handler.backend_id(), caps);
        encode_and_write(&mut writer, &hello)?;

        // 2. Read frames until we get a Run
        let (run_id, work_order) = loop {
            let mut line = String::new();
            let n = reader.read_line(&mut line).map_err(HarnessError::Io)?;
            if n == 0 {
                return Ok(()); // clean EOF
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let frame: Frame = JsonlCodec::decode(trimmed)
                .map_err(|e| HarnessError::Protocol(format!("failed to decode frame: {e}")))?;
            match frame {
                Frame::Run { id, work_order } => break (id, work_order),
                Frame::Cancel { .. } => return Ok(()),
                Frame::Ping { seq } => {
                    encode_and_write(&mut writer, &Frame::Pong { seq })?;
                }
                other => {
                    return Err(HarnessError::Protocol(format!(
                        "expected run frame, got: {other:?}"
                    )));
                }
            }
        };

        // 3. Execute handler, buffering event frames
        let event_buf = Arc::new(Mutex::new(Vec::<u8>::new()));

        let ctx = HandlerContext {
            run_id: run_id.clone(),
            work_order,
            event_buf: event_buf.clone(),
        };

        let result = self.handler.handle_run(ctx);

        // Flush buffered event frames to the real writer
        {
            let buf = event_buf.lock().unwrap();
            writer.write_all(&buf).map_err(HarnessError::Io)?;
        }

        // 4. Send final or fatal
        match result {
            Ok(receipt) => {
                let frame = Frame::Final {
                    ref_id: run_id,
                    receipt,
                };
                encode_and_write(&mut writer, &frame)?;
            }
            Err(msg) => {
                let frame = Frame::Fatal {
                    ref_id: Some(run_id),
                    error: msg,
                };
                encode_and_write(&mut writer, &frame)?;
            }
        }

        writer.flush().map_err(HarnessError::Io)?;
        Ok(())
    }

    /// Run the protocol loop over stdin/stdout.
    ///
    /// Convenience wrapper around [`run`](Self::run) using locked stdio.
    pub fn run_stdio(&self) -> Result<(), HarnessError> {
        let stdin = io::stdin();
        let stdout = io::stdout();
        self.run(stdin.lock(), stdout.lock())
    }
}

// ── Internal helpers ────────────────────────────────────────────────

fn hello_frame_with_caps(backend_id: &str, caps: CapabilitySet) -> Frame {
    Frame::Hello {
        contract_version: "abp/v0.1".to_string(),
        backend: serde_json::json!({ "id": backend_id }),
        capabilities: caps.build(),
        mode: Value::Null,
    }
}

fn encode_and_write<W: Write>(writer: &mut W, frame: &Frame) -> Result<(), HarnessError> {
    let line =
        JsonlCodec::encode(frame).map_err(|e| HarnessError::Protocol(format!("encode: {e}")))?;
    writer
        .write_all(line.as_bytes())
        .map_err(HarnessError::Io)?;
    writer.flush().map_err(HarnessError::Io)?;
    Ok(())
}

// ── Error type ──────────────────────────────────────────────────────

/// Errors from the sidecar harness.
#[derive(Debug)]
pub enum HarnessError {
    /// I/O error on stdin/stdout.
    Io(io::Error),
    /// JSONL protocol violation.
    Protocol(String),
}

impl std::fmt::Display for HarnessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Protocol(msg) => write!(f, "protocol error: {msg}"),
        }
    }
}

impl std::error::Error for HarnessError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}
