// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
//! abp-integrations
#![deny(unsafe_code)]
#![warn(missing_docs)]
//!
//! Compatibility facade over backend microcrates.

pub mod capability;
pub mod health;
pub mod metrics;
pub mod projection;
pub mod selector;

pub use abp_backend_core::{
    Backend, ensure_capability_requirements, extract_execution_mode,
    validate_passthrough_compatibility,
};
pub use abp_backend_mock::MockBackend;
pub use abp_backend_sidecar::SidecarBackend;
