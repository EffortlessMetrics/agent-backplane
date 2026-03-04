// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
//! Reusable sidecar protocol utilities for Agent Backplane.
//!
//! This crate provides higher-level building blocks on top of
//! [`abp_protocol`] for implementing sidecar hosts and clients:
//!
//! - [`codec::StreamingCodec`] — Enhanced JSONL codec with chunked reading,
//!   line-length limits, error recovery, and metrics.
//! - [`handshake::HandshakeManager`] — Async hello handshake with timeout
//!   and contract-version validation.
//! - [`event_stream::EventStreamProcessor`] — Validates ref_id correlation,
//!   detects out-of-order events, produces typed event streams.
//! - [`health::ProtocolHealth`] — Heartbeat, timeout detection, and graceful
//!   shutdown signaling.
//! - [`process::SidecarProcess`] — Process management helpers for sidecar
//!   child processes.
//! - [`stdio::StdioPipes`] — Stdio pipe setup and buffered I/O wrappers.
//! - [`timeout::TimeoutManager`] — Per-phase timeout management for the
//!   sidecar protocol lifecycle.
//! - [`discovery::SidecarRegistry`] — Sidecar discovery and registration.
#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod codec;
pub mod discovery;
pub mod event_stream;
pub mod frame;
pub mod handshake;
pub mod health;
pub mod process;
pub mod stdio;
pub mod testing;
pub mod timeout;
pub mod validate;

// Re-export the most commonly used items for convenience.
pub use frame::{
    decode_envelope, encode_envelope, encode_event, encode_fatal, encode_final, encode_hello,
};
pub use testing::{mock_event, mock_fatal, mock_final, mock_hello, mock_work_order};
pub use validate::{validate_hello, validate_ref_id, validate_sequence};
