// SPDX-License-Identifier: MIT OR Apache-2.0
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

/// Configuration for a [`ClaudeBridge`](crate::ClaudeBridge) instance.
#[derive(Debug, Clone)]
pub struct ClaudeBridgeConfig {
    /// Override the Node.js binary name or path (default: auto-detected).
    pub node_command: Option<String>,
    /// Path to the host sidecar script (default: auto-discovered).
    pub host_script: Option<PathBuf>,
    /// Extra environment variables passed to the sidecar process.
    pub env: BTreeMap<String, String>,
    /// Working directory for the sidecar process.
    pub cwd: Option<PathBuf>,
    /// Optional JS adapter module injected via `ABP_CLAUDE_ADAPTER_MODULE`.
    pub adapter_module: Option<PathBuf>,
    /// Maximum time to wait for the sidecar `hello` handshake.
    pub handshake_timeout: Duration,
    /// Capacity of the internal event channel.
    pub channel_buffer: usize,
}

impl Default for ClaudeBridgeConfig {
    fn default() -> Self {
        Self {
            node_command: None,
            host_script: None,
            env: BTreeMap::new(),
            cwd: None,
            adapter_module: None,
            handshake_timeout: Duration::from_secs(30),
            channel_buffer: 256,
        }
    }
}

impl ClaudeBridgeConfig {
    /// Create a default configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the Anthropic API key (`ANTHROPIC_API_KEY` env var).
    pub fn with_api_key(mut self, key: impl Into<String>) -> Self {
        self.env.insert("ANTHROPIC_API_KEY".into(), key.into());
        self
    }

    /// Set the path to the host sidecar script.
    pub fn with_host_script(mut self, path: impl Into<PathBuf>) -> Self {
        self.host_script = Some(path.into());
        self
    }

    /// Set the working directory for the sidecar process.
    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Set a custom JS adapter module for the sidecar.
    pub fn with_adapter_module(mut self, path: impl Into<PathBuf>) -> Self {
        self.adapter_module = Some(path.into());
        self
    }

    /// Override the Node.js binary name or path.
    pub fn with_node_command(mut self, cmd: impl Into<String>) -> Self {
        self.node_command = Some(cmd.into());
        self
    }

    /// Add an environment variable for the sidecar process.
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Set the handshake timeout duration.
    pub fn with_handshake_timeout(mut self, timeout: Duration) -> Self {
        self.handshake_timeout = timeout;
        self
    }

    /// Set the event channel buffer capacity.
    pub fn with_channel_buffer(mut self, size: usize) -> Self {
        self.channel_buffer = size;
        self
    }
}
