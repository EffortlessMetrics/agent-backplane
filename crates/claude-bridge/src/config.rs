use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ClaudeBridgeConfig {
    pub node_command: Option<String>,
    pub host_script: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
    pub cwd: Option<PathBuf>,
    pub adapter_module: Option<PathBuf>,
    pub handshake_timeout: Duration,
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
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_api_key(mut self, key: impl Into<String>) -> Self {
        self.env.insert("ANTHROPIC_API_KEY".into(), key.into());
        self
    }

    pub fn with_host_script(mut self, path: impl Into<PathBuf>) -> Self {
        self.host_script = Some(path.into());
        self
    }

    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    pub fn with_adapter_module(mut self, path: impl Into<PathBuf>) -> Self {
        self.adapter_module = Some(path.into());
        self
    }

    pub fn with_node_command(mut self, cmd: impl Into<String>) -> Self {
        self.node_command = Some(cmd.into());
        self
    }

    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    pub fn with_handshake_timeout(mut self, timeout: Duration) -> Self {
        self.handshake_timeout = timeout;
        self
    }

    pub fn with_channel_buffer(mut self, size: usize) -> Self {
        self.channel_buffer = size;
        self
    }
}
