// SPDX-License-Identifier: MIT OR Apache-2.0
//! Process management utilities for sidecar lifecycle tracking.

use crate::SidecarSpec;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

/// Configuration for spawning a managed sidecar process.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProcessConfig {
    /// Working directory for the process.
    pub working_dir: Option<PathBuf>,
    /// Additional environment variables to set.
    pub env_vars: BTreeMap<String, String>,
    /// Maximum time the process is allowed to run before being killed.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "option_duration_millis"
    )]
    pub timeout: Option<Duration>,
    /// Whether to inherit the parent process's environment variables.
    #[serde(default = "default_true")]
    pub inherit_env: bool,
}

fn default_true() -> bool {
    true
}

/// Serde helper for `Option<Duration>` as milliseconds.
mod option_duration_millis {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(val: &Option<Duration>, ser: S) -> Result<S::Ok, S::Error> {
        match val {
            Some(d) => d.as_millis().serialize(ser),
            None => ser.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<Option<Duration>, D::Error> {
        let opt: Option<u64> = Option::deserialize(de)?;
        Ok(opt.map(Duration::from_millis))
    }
}

impl Default for ProcessConfig {
    fn default() -> Self {
        Self {
            working_dir: None,
            env_vars: BTreeMap::new(),
            timeout: None,
            inherit_env: true,
        }
    }
}

/// Runtime status of a managed sidecar process.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ProcessStatus {
    /// The process has not been started yet.
    NotStarted,
    /// The process is currently running.
    Running {
        /// OS process identifier.
        pid: u32,
    },
    /// The process exited normally with the given code.
    Exited {
        /// Exit code returned by the process.
        code: i32,
    },
    /// The process was forcefully killed.
    Killed,
    /// The process exceeded its configured timeout and was terminated.
    TimedOut,
}

/// Tracks the full lifecycle of a managed sidecar process.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProcessInfo {
    /// Specification used to spawn the sidecar.
    pub spec: SidecarSpec,
    /// Configuration applied to the process.
    pub config: ProcessConfig,
    /// Current status of the process.
    pub status: ProcessStatus,
    /// When the process was started.
    pub started_at: Option<DateTime<Utc>>,
    /// When the process ended (exited, killed, or timed out).
    pub ended_at: Option<DateTime<Utc>>,
}

impl ProcessInfo {
    /// Create a new `ProcessInfo` in the `NotStarted` state.
    pub fn new(spec: SidecarSpec, config: ProcessConfig) -> Self {
        Self {
            spec,
            config,
            status: ProcessStatus::NotStarted,
            started_at: None,
            ended_at: None,
        }
    }

    /// Returns `true` if the process is currently running.
    pub fn is_running(&self) -> bool {
        matches!(self.status, ProcessStatus::Running { .. })
    }

    /// Returns `true` if the process has terminated (exited, killed, or timed out).
    pub fn is_terminated(&self) -> bool {
        matches!(
            self.status,
            ProcessStatus::Exited { .. } | ProcessStatus::Killed | ProcessStatus::TimedOut
        )
    }
}
