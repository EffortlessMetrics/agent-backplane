// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
//! sidecar-kit
#![deny(unsafe_code)]
//!
//! Value-based transport layer for sidecar processes speaking JSONL over stdio.
//!
//! This crate provides the low-level building blocks for spawning and communicating
//! with sidecar processes that speak the ABP JSONL protocol. All payload fields use
//! `serde_json::Value`, making this crate independent of `abp-core` types.

pub mod cancel;
pub mod client;
pub mod codec;
pub mod error;
pub mod frame;
pub mod middleware;
pub mod process;
pub mod run;
pub mod spec;

pub use cancel::CancelToken;
pub use client::{HelloData, SidecarClient};
pub use codec::JsonlCodec;
pub use error::SidecarError;
pub use frame::Frame;
pub use middleware::{EventMiddleware, FilterMiddleware, LoggingMiddleware, MiddlewareChain};
pub use process::SidecarProcess;
pub use run::RawRun;
pub use spec::ProcessSpec;
