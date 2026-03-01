// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
//! sidecar-kit
#![deny(unsafe_code)]
#![warn(missing_docs)]
//!
//! Value-based transport layer for sidecar processes speaking JSONL over stdio.
//!
//! This crate provides the low-level building blocks for spawning and communicating
//! with sidecar processes that speak the ABP JSONL protocol. All payload fields use
//! `serde_json::Value`, making this crate independent of `abp-core` types.

pub mod builders;
pub mod cancel;
pub mod client;
pub mod codec;
pub mod diagnostics;
pub mod error;
pub mod frame;
pub mod middleware;
pub mod pipeline;
pub mod process;
pub mod run;
pub mod spec;
pub mod transform;

pub use builders::{
    ReceiptBuilder, event_command_executed, event_error, event_file_changed, event_frame,
    event_run_completed, event_run_started, event_text_delta, event_text_message, event_tool_call,
    event_tool_result, event_warning, fatal_frame, hello_frame,
};
pub use cancel::CancelToken;
pub use client::{HelloData, SidecarClient};
pub use codec::JsonlCodec;
pub use error::SidecarError;
pub use frame::Frame;
pub use middleware::{
    ErrorWrapMiddleware, EventMiddleware, FilterMiddleware, LoggingMiddleware, MiddlewareChain,
    TimingMiddleware,
};
pub use pipeline::{
    EventPipeline, PipelineError, PipelineStage, RedactStage, TimestampStage, ValidateStage,
};
pub use process::SidecarProcess;
pub use run::RawRun;
pub use spec::ProcessSpec;
pub use transform::{
    EnrichTransformer, EventTransformer, FilterTransformer, RedactTransformer, ThrottleTransformer,
    TimestampTransformer, TransformerChain,
};
