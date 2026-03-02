// SPDX-License-Identifier: MIT OR Apache-2.0
//! Backwards-compatible compression module re-export.
//!
//! Compression primitives now live in the dedicated `abp-compress` crate.
//! This module re-exports that API to preserve `abp_protocol::compress::*`
//! paths.

pub use abp_compress::*;
