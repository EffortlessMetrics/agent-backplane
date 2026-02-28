// SPDX-License-Identifier: MIT OR Apache-2.0
//! abp-host
#![deny(unsafe_code)]
//!
//! Process supervision + JSONL transport for sidecars.

pub mod registry;

use abp_core::{AgentEvent, BackendIdentity, CapabilityManifest, Receipt, WorkOrder};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::process::Stdio;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::ReceiverStream;
use tracing::{debug, warn};

/// Configuration for spawning a sidecar process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidecarSpec {
    /// Executable command to run.
    pub command: String,
    /// Arguments passed to the command.
    pub args: Vec<String>,
    /// Environment variables set for the process.
    pub env: BTreeMap<String, String>,
    /// Working directory for the process.
    pub cwd: Option<String>,
}

impl SidecarSpec {
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

/// Data extracted from a sidecar's initial `hello` handshake.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidecarHello {
    /// Protocol version reported by the sidecar.
    pub contract_version: String,
    /// Backend identity metadata from the sidecar.
    pub backend: BackendIdentity,
    /// Capability manifest advertised by the sidecar.
    pub capabilities: CapabilityManifest,
}

/// A connected sidecar process that has completed its `hello` handshake.
///
/// Use [`SidecarClient::spawn`] to create, then [`SidecarClient::run`] to execute a work order.
#[derive(Debug)]
pub struct SidecarClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<tokio::process::ChildStdout>,
    pub hello: SidecarHello,
}

/// An in-progress sidecar run: provides an event stream, a receipt future, and a wait handle.
#[derive(Debug)]
pub struct SidecarRun {
    /// Stream of normalized events.
    pub events: ReceiverStream<AgentEvent>,

    /// Final receipt for the run.
    pub receipt: oneshot::Receiver<Result<Receipt, HostError>>,

    /// Wait for the underlying sidecar process to exit.
    pub wait: tokio::task::JoinHandle<Result<(), HostError>>,
}

/// Errors from sidecar process management and protocol handling.
#[derive(Debug, Error)]
pub enum HostError {
    #[error("failed to spawn sidecar: {0}")]
    Spawn(#[source] std::io::Error),

    #[error("failed to read sidecar stdout: {0}")]
    Stdout(#[source] std::io::Error),

    #[error("failed to write sidecar stdin: {0}")]
    Stdin(#[source] std::io::Error),

    #[error("protocol error: {0}")]
    Protocol(#[from] ProtocolError),

    #[error("sidecar protocol violation: {0}")]
    Violation(String),

    #[error("sidecar fatal error: {0}")]
    Fatal(String),

    #[error("sidecar exited unexpectedly (code={code:?})")]
    Exited { code: Option<i32> },
}

impl SidecarClient {
    /// Spawn a sidecar process and perform the `hello` handshake.
    ///
    /// The sidecar MUST emit a `hello` envelope as its first stdout line.
    pub async fn spawn(spec: SidecarSpec) -> Result<Self, HostError> {
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

        let mut child = cmd.spawn().map_err(HostError::Spawn)?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| HostError::Violation("sidecar stdin unavailable".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| HostError::Violation("sidecar stdout unavailable".into()))?;

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
                                warn!(target: "abp.sidecar.stderr", "{s}");
                            }
                        }
                        Err(_) => break,
                    }
                }
            });
        }

        let mut stdout = BufReader::new(stdout);

        // Expect hello as the first line.
        let mut line = String::new();
        let n = stdout
            .read_line(&mut line)
            .await
            .map_err(HostError::Stdout)?;
        if n == 0 {
            let status = child.wait().await.ok();
            return Err(HostError::Exited {
                code: status.and_then(|s| s.code()),
            });
        }

        let env = JsonlCodec::decode(line.trim_end())?;
        let (contract_version, backend, capabilities) = match env {
            Envelope::Hello {
                contract_version,
                backend,
                capabilities,
                ..
            } => (contract_version, backend, capabilities),
            other => {
                return Err(HostError::Protocol(ProtocolError::UnexpectedMessage {
                    expected: "hello".into(),
                    got: format!("{other:?}"),
                }));
            }
        };

        debug!(target: "abp.sidecar", "sidecar hello: backend={}", backend.id);

        Ok(Self {
            child,
            stdin,
            stdout,
            hello: SidecarHello {
                contract_version,
                backend,
                capabilities,
            },
        })
    }

    /// Send a work order and begin streaming events from the sidecar.
    ///
    /// Consumes `self` because a single client handles exactly one run.
    pub async fn run(
        mut self,
        run_id: String,
        work_order: WorkOrder,
    ) -> Result<SidecarRun, HostError> {
        let (ev_tx, ev_rx) = mpsc::channel::<AgentEvent>(256);
        let (receipt_tx, receipt_rx) = oneshot::channel::<Result<Receipt, HostError>>();

        // Send Run request.
        let msg = Envelope::Run {
            id: run_id.clone(),
            work_order,
        };
        let line = JsonlCodec::encode(&msg)?;
        self.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(HostError::Stdin)?;
        self.stdin.flush().await.map_err(HostError::Stdin)?;

        let mut stdout = self.stdout;
        let mut child = self.child;

        let wait = tokio::spawn(async move {
            let mut buf = String::new();
            loop {
                buf.clear();
                let n = stdout
                    .read_line(&mut buf)
                    .await
                    .map_err(HostError::Stdout)?;
                if n == 0 {
                    // Child closed stdout; treat as exit.
                    let status = child.wait().await.map_err(|e| HostError::Exited {
                        code: e.raw_os_error(),
                    })?;
                    return Err(HostError::Exited {
                        code: status.code(),
                    });
                }

                let line = buf.trim_end();
                if line.is_empty() {
                    continue;
                }

                match JsonlCodec::decode(line) {
                    Ok(Envelope::Event { ref_id, event }) => {
                        if ref_id != run_id {
                            warn!(target: "abp.sidecar", "dropping event for other run_id={ref_id}");
                            continue;
                        }
                        if ev_tx.send(event).await.is_err() {
                            // Receiver dropped; stop.
                            break;
                        }
                    }
                    Ok(Envelope::Final { ref_id, receipt }) => {
                        if ref_id != run_id {
                            warn!(target: "abp.sidecar", "dropping final for other run_id={ref_id}");
                            continue;
                        }
                        let _ = receipt_tx.send(Ok(receipt));
                        break;
                    }
                    Ok(Envelope::Fatal { ref_id, error }) => {
                        if let Some(ref_id) = ref_id
                            && ref_id != run_id
                        {
                            warn!(target: "abp.sidecar", "dropping fatal for other run_id={ref_id}");
                            continue;
                        }
                        let _ = receipt_tx.send(Err(HostError::Fatal(error.clone())));
                        break;
                    }
                    Ok(Envelope::Hello { .. }) => {
                        // Ignore; handshake already done.
                        continue;
                    }
                    Ok(other) => {
                        let _ = receipt_tx.send(Err(HostError::Violation(format!(
                            "unexpected message: {other:?}"
                        ))));
                        break;
                    }
                    Err(e) => {
                        let _ = receipt_tx.send(Err(HostError::Protocol(e)));
                        break;
                    }
                }
            }

            // Ensure the child is waited on. If it's still alive, terminate.
            let _ = child.kill().await;
            let _ = child.wait().await;
            Ok(())
        });

        Ok(SidecarRun {
            events: ReceiverStream::new(ev_rx),
            receipt: receipt_rx,
            wait,
        })
    }
}

/// Type-erased stream of [`AgentEvent`]s.
// Convenience: accept a stream of events as a trait object.
pub type EventStream = dyn Stream<Item = AgentEvent> + Send + Unpin;

// Re-export raw transport types from sidecar-kit for advanced protocol usage.
/// Low-level cancellation token for sidecar processes.
pub use sidecar_kit::CancelToken;
/// Raw process specification for spawning sidecar processes.
pub use sidecar_kit::ProcessSpec as RawProcessSpec;
/// Raw run handle from the sidecar-kit transport layer.
pub use sidecar_kit::RawRun;
/// Raw sidecar client from the sidecar-kit transport layer.
pub use sidecar_kit::SidecarClient as RawSidecarClient;
/// Errors from the sidecar-kit transport layer.
pub use sidecar_kit::SidecarError;
/// Raw sidecar process handle from the sidecar-kit transport layer.
pub use sidecar_kit::SidecarProcess as RawSidecarProcess;
