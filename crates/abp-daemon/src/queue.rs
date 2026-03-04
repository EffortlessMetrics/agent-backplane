// SPDX-License-Identifier: MIT OR Apache-2.0
//! Priority-based run queue for the ABP daemon.
//!
//! This module re-exports the queue primitives from [`abp_run_queue`] for
//! backward compatibility.

pub use abp_run_queue::{QueueError, QueuePriority, QueueStats, QueuedRun, RunQueue};
