// SPDX-License-Identifier: MIT OR Apache-2.0
//! Process specification types.

use std::collections::BTreeMap;

/// Configuration for spawning a sidecar process (command, args, env, cwd).
#[derive(Debug, Clone)]
pub struct ProcessSpec {
    /// Executable command to run.
    pub command: String,
    /// Arguments passed to the command.
    pub args: Vec<String>,
    /// Additional environment variables for the process.
    pub env: BTreeMap<String, String>,
    /// Optional working directory override.
    pub cwd: Option<String>,
}

impl ProcessSpec {
    /// Create a spec with the given command and default (empty) args/env.
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            args: Vec::new(),
            env: BTreeMap::new(),
            cwd: None,
        }
    }
}
