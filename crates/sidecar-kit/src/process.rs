// SPDX-License-Identifier: MIT OR Apache-2.0
//! Low-level process spawning and stdio management.

use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tracing::warn;

use super::{Frame, JsonlCodec, ProcessSpec, SidecarError};

/// A spawned sidecar process with captured stdin/stdout for JSONL communication.
pub struct SidecarProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<tokio::process::ChildStdout>,
}

impl SidecarProcess {
    /// Spawn a new sidecar from the given process specification.
    ///
    /// Stderr is forwarded through `tracing` at warn level.
    pub async fn spawn(spec: ProcessSpec) -> Result<Self, SidecarError> {
        let mut cmd = Command::new(&spec.command);
        cmd.args(&spec.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(cwd) = &spec.cwd {
            cmd.current_dir(cwd);
        }

        for (k, v) in &spec.env {
            cmd.env(k, v);
        }

        let mut child = cmd.spawn().map_err(SidecarError::Spawn)?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| SidecarError::Protocol("stdin unavailable".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| SidecarError::Protocol("stdout unavailable".into()))?;

        // Forward stderr via tracing
        let stderr = child.stderr.take();
        if let Some(stderr) = stderr {
            tokio::spawn(async move {
                let mut r = BufReader::new(stderr);
                let mut line = String::new();
                loop {
                    line.clear();
                    match r.read_line(&mut line).await {
                        Ok(0) => break,
                        Ok(_) => {
                            let s = line.trim_end();
                            if !s.is_empty() {
                                warn!(target: "sidecar_kit.stderr", "{s}");
                            }
                        }
                        Err(_) => break,
                    }
                }
            });
        }

        let stdout = BufReader::new(stdout);

        Ok(Self {
            child,
            stdin,
            stdout,
        })
    }

    /// Send a frame to the sidecar's stdin.
    pub async fn send(&mut self, frame: &Frame) -> Result<(), SidecarError> {
        let line = JsonlCodec::encode(frame)?;
        self.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(SidecarError::Stdin)?;
        self.stdin.flush().await.map_err(SidecarError::Stdin)?;
        Ok(())
    }

    /// Read the next frame from the sidecar's stdout, or `None` on EOF.
    pub async fn recv(&mut self) -> Result<Option<Frame>, SidecarError> {
        let mut buf = String::new();
        let n = self
            .stdout
            .read_line(&mut buf)
            .await
            .map_err(SidecarError::Stdout)?;
        if n == 0 {
            return Ok(None);
        }
        let line = buf.trim_end();
        if line.is_empty() {
            return Ok(None);
        }
        JsonlCodec::decode(line).map(Some)
    }

    /// Kill the sidecar process and wait for it to exit.
    pub async fn kill(&mut self) {
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
    }

    /// Consume self and return the inner parts for manual management.
    pub fn into_parts(self) -> (Child, ChildStdin, BufReader<tokio::process::ChildStdout>) {
        (self.child, self.stdin, self.stdout)
    }
}
