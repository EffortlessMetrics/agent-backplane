// SPDX-License-Identifier: MIT OR Apache-2.0
//! Process management helpers for sidecar child processes.
//!
//! [`SidecarProcess`] wraps a [`tokio::process::Child`] with helpers for
//! graceful shutdown, exit-code inspection, and process lifecycle tracking.

use std::path::PathBuf;
use std::time::Duration;

use thiserror::Error;
use tokio::process::{Child, Command};
use tracing::{debug, warn};

/// Default grace period before force-killing a sidecar (5 seconds).
pub const DEFAULT_KILL_TIMEOUT: Duration = Duration::from_secs(5);

/// Errors from sidecar process management.
#[derive(Debug, Error)]
pub enum ProcessError {
    /// Failed to spawn the sidecar process.
    #[error("failed to spawn sidecar: {0}")]
    Spawn(#[source] std::io::Error),
    /// The process exited with a non-zero status.
    #[error("sidecar exited with status {0}")]
    NonZeroExit(i32),
    /// The process was terminated by a signal (Unix) or had no exit code.
    #[error("sidecar terminated without exit code")]
    NoExitCode,
    /// Timed out waiting for the process to exit.
    #[error("timed out waiting for sidecar to exit")]
    Timeout,
    /// I/O error interacting with the process.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// State of a managed sidecar process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    /// The process has not been started yet.
    NotStarted,
    /// The process is currently running.
    Running,
    /// The process exited normally.
    Exited(i32),
    /// The process was killed or terminated without an exit code.
    Killed,
}

impl ProcessState {
    /// Whether the process is still alive.
    #[must_use]
    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running)
    }
}

/// Configuration for spawning a sidecar process.
#[derive(Debug, Clone)]
pub struct SidecarProcessConfig {
    /// Path to the executable.
    pub program: PathBuf,
    /// Arguments to pass.
    pub args: Vec<String>,
    /// Working directory (optional).
    pub working_dir: Option<PathBuf>,
    /// Grace period before force-killing on shutdown.
    pub kill_timeout: Duration,
}

impl SidecarProcessConfig {
    /// Create a config for the given program path.
    #[must_use]
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            working_dir: None,
            kill_timeout: DEFAULT_KILL_TIMEOUT,
        }
    }

    /// Add command-line arguments.
    #[must_use]
    pub fn args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }

    /// Set the working directory.
    #[must_use]
    pub fn working_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.working_dir = Some(dir.into());
        self
    }

    /// Set the kill timeout.
    #[must_use]
    pub fn kill_timeout(mut self, timeout: Duration) -> Self {
        self.kill_timeout = timeout;
        self
    }
}

/// A managed sidecar child process.
///
/// Wraps [`tokio::process::Child`] with lifecycle helpers.
pub struct SidecarProcess {
    child: Option<Child>,
    config: SidecarProcessConfig,
    state: ProcessState,
}

impl SidecarProcess {
    /// Spawn a new sidecar process from the given config.
    ///
    /// Stdio pipes are configured for protocol communication: stdin and
    /// stdout are piped, stderr is piped for capture.
    pub fn spawn(config: SidecarProcessConfig) -> Result<Self, ProcessError> {
        let mut cmd = Command::new(&config.program);
        cmd.args(&config.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        if let Some(dir) = &config.working_dir {
            cmd.current_dir(dir);
        }

        debug!(program = %config.program.display(), "spawning sidecar process");
        let child = cmd.spawn().map_err(ProcessError::Spawn)?;

        Ok(Self {
            child: Some(child),
            config,
            state: ProcessState::Running,
        })
    }

    /// The current process state.
    #[must_use]
    pub fn state(&self) -> ProcessState {
        self.state
    }

    /// The process configuration.
    #[must_use]
    pub fn config(&self) -> &SidecarProcessConfig {
        &self.config
    }

    /// Take the child's stdin handle (can only be called once).
    pub fn take_stdin(&mut self) -> Option<tokio::process::ChildStdin> {
        self.child.as_mut()?.stdin.take()
    }

    /// Take the child's stdout handle (can only be called once).
    pub fn take_stdout(&mut self) -> Option<tokio::process::ChildStdout> {
        self.child.as_mut()?.stdout.take()
    }

    /// Take the child's stderr handle (can only be called once).
    pub fn take_stderr(&mut self) -> Option<tokio::process::ChildStderr> {
        self.child.as_mut()?.stderr.take()
    }

    /// The OS process ID, if the process is still alive.
    #[must_use]
    pub fn pid(&self) -> Option<u32> {
        self.child.as_ref()?.id()
    }

    /// Wait for the process to exit and update the internal state.
    pub async fn wait(&mut self) -> Result<ProcessState, ProcessError> {
        if let Some(child) = self.child.as_mut() {
            let status = child.wait().await?;
            self.state = match status.code() {
                Some(code) => ProcessState::Exited(code),
                None => ProcessState::Killed,
            };
            Ok(self.state)
        } else {
            Ok(self.state)
        }
    }

    /// Attempt a graceful shutdown: wait for the configured kill timeout,
    /// then force-kill if the process hasn't exited.
    pub async fn shutdown(&mut self) -> Result<ProcessState, ProcessError> {
        let Some(child) = self.child.as_mut() else {
            return Ok(self.state);
        };

        let timeout = self.config.kill_timeout;
        match tokio::time::timeout(timeout, child.wait()).await {
            Ok(Ok(status)) => {
                self.state = match status.code() {
                    Some(code) => ProcessState::Exited(code),
                    None => ProcessState::Killed,
                };
            }
            _ => {
                warn!("sidecar did not exit within {:?}, killing", timeout);
                let _ = child.kill().await;
                self.state = ProcessState::Killed;
            }
        }

        Ok(self.state)
    }

    /// Force-kill the process immediately.
    pub async fn kill(&mut self) -> Result<(), ProcessError> {
        if let Some(child) = self.child.as_mut() {
            child.kill().await?;
            self.state = ProcessState::Killed;
        }
        Ok(())
    }
}

impl std::fmt::Debug for SidecarProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SidecarProcess")
            .field("program", &self.config.program)
            .field("state", &self.state)
            .field("pid", &self.pid())
            .finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_state_is_running() {
        assert!(ProcessState::Running.is_running());
        assert!(!ProcessState::NotStarted.is_running());
        assert!(!ProcessState::Exited(0).is_running());
        assert!(!ProcessState::Killed.is_running());
    }

    #[test]
    fn config_builder() {
        let cfg = SidecarProcessConfig::new("node")
            .args(["script.js", "--flag"])
            .working_dir("/tmp")
            .kill_timeout(Duration::from_secs(10));
        assert_eq!(cfg.program, PathBuf::from("node"));
        assert_eq!(cfg.args, vec!["script.js", "--flag"]);
        assert_eq!(cfg.working_dir, Some(PathBuf::from("/tmp")));
        assert_eq!(cfg.kill_timeout, Duration::from_secs(10));
    }

    #[test]
    fn config_defaults() {
        let cfg = SidecarProcessConfig::new("python");
        assert!(cfg.args.is_empty());
        assert!(cfg.working_dir.is_none());
        assert_eq!(cfg.kill_timeout, DEFAULT_KILL_TIMEOUT);
    }

    #[test]
    fn spawn_nonexistent_returns_error() {
        let cfg = SidecarProcessConfig::new("__nonexistent_binary_12345__");
        let result = SidecarProcess::spawn(cfg);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ProcessError::Spawn(_)));
        assert!(err.to_string().contains("spawn"));
    }

    #[test]
    fn error_display_messages() {
        let e = ProcessError::NonZeroExit(1);
        assert!(e.to_string().contains("status 1"));

        let e = ProcessError::NoExitCode;
        assert!(e.to_string().contains("without exit code"));

        let e = ProcessError::Timeout;
        assert!(e.to_string().contains("timed out"));
    }
}
