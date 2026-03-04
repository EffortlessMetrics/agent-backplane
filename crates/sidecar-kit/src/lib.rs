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
pub mod capabilities;
pub mod client;
pub mod codec;
pub mod diagnostics;
pub mod error;
pub mod events;
pub mod frame;
pub mod framing;
pub mod harness;
pub mod middleware;
pub mod pipeline;
pub mod process;
pub mod protocol_helpers;
pub mod protocol_state;
pub mod receipt_builder;
pub mod run;
pub mod spec;
pub mod test_utils;
pub mod transform;
pub mod typed_middleware;
pub mod work_order;

pub use builders::{
    EventBuilder, ReceiptBuilder, event_command_executed, event_error, event_file_changed,
    event_frame, event_run_completed, event_run_started, event_text_delta, event_text_message,
    event_tool_call, event_tool_result, event_warning, fatal_frame, final_frame, hello_frame,
};
pub use cancel::CancelToken;
pub use client::{HelloData, SidecarClient};
pub use codec::JsonlCodec;
pub use error::SidecarError;
pub use frame::Frame;
pub use framing::{
    FrameReader, FrameValidation, FrameWriter, buf_reader_from_bytes, frame_to_json, json_to_frame,
    read_all_frames, validate_frame, write_frames,
};
pub use middleware::{
    ErrorWrapMiddleware, EventMiddleware, FilterMiddleware, LoggingMiddleware, MiddlewareChain,
    TimingMiddleware,
};
pub use pipeline::{
    EventPipeline, PipelineError, PipelineStage, RedactStage, TimestampStage, ValidateStage,
};
pub use process::SidecarProcess;
pub use protocol_state::{ProtocolPhase, ProtocolState};
pub use run::RawRun;
pub use spec::ProcessSpec;
pub use transform::{
    EnrichTransformer, EventTransformer, FilterTransformer, RedactTransformer, ThrottleTransformer,
    TimestampTransformer, TransformerChain,
};
pub use typed_middleware::{
    ErrorRecoveryMiddleware as TypedErrorRecoveryMiddleware, MetricsMiddleware, MiddlewareAction,
    RateLimitMiddleware, SidecarMiddleware, SidecarMiddlewareChain,
};

pub use events::{
    EventBuilder as TypedEventBuilder, command_event, delta_event, error_event, file_changed_event,
    run_completed_event, run_started_event, text_event, tool_call_event, tool_result_event,
    warning_event,
};
pub use protocol_helpers::{read_run, send_event, send_fatal, send_final, send_hello};
pub use receipt_builder::TypedReceiptBuilder;
pub use test_utils::{
    MockStdin, MockStdout, SidecarTestHarness, assert_valid_event, assert_valid_fatal,
    assert_valid_final, assert_valid_hello,
};

pub use capabilities::{CapabilitySet, default_streaming_capabilities};
pub use harness::{HandlerContext, HarnessError, SidecarHandler, SidecarHarness};
pub use work_order::WorkOrderView;
