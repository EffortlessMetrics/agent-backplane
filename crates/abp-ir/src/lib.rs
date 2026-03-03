// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
#![warn(missing_docs)]
#![deny(unsafe_code)]
//! Re-exports of the cross-dialect IR types from [`abp_core::ir`].
//!
//! This crate provides a focused entry point for the intermediate
//! representation layer, keeping the heavy property-based test suite
//! isolated from `abp-core` itself.

pub use abp_core::ir::*;
