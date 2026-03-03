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
#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod codec;
pub mod event_stream;
pub mod handshake;
pub mod health;
