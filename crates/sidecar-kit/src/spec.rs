//! Process specification types.

use std::collections::BTreeMap;

/// Configuration for spawning a sidecar process (command, args, env, cwd).
#[derive(Debug, Clone)]
pub struct ProcessSpec {
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
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
