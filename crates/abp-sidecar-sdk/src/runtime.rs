// SPDX-License-Identifier: MIT OR Apache-2.0
//! Sidecar runtime — manages the JSONL I/O lifecycle over stdin/stdout.
//!
//! [`SidecarRuntime`] is produced by [`SidecarBuilder::build`] and handles:
//!
//! - Sending the `hello` handshake automatically
//! - Reading `run` envelopes from stdin
//! - Delegating to the registered run handler
//! - Streaming events back as `event` envelopes
//! - Sending the terminal `final` or `fatal` envelope
//! - Graceful shutdown
//!
//! [`SidecarBuilder::build`]: crate::builder::SidecarBuilder::build

use abp_core::{BackendIdentity, CapabilityManifest, ExecutionMode, WorkOrder};
use abp_protocol::{Envelope, JsonlCodec};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::builder::{RunHandler, SidecarError};
use crate::emitter::EventEmitter;

/// Manages the sidecar protocol lifecycle.
///
/// Created via [`SidecarBuilder::build`](crate::builder::SidecarBuilder::build).
pub struct SidecarRuntime {
    identity: BackendIdentity,
    capabilities: CapabilityManifest,
    mode: ExecutionMode,
    handler: RunHandler,
}

impl std::fmt::Debug for SidecarRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SidecarRuntime")
            .field("identity", &self.identity)
            .field("capabilities", &self.capabilities)
            .field("mode", &self.mode)
            .field("handler", &"<RunHandler>")
            .finish()
    }
}

impl SidecarRuntime {
    /// Create a new runtime. Prefer [`SidecarBuilder`](crate::builder::SidecarBuilder).
    pub(crate) fn new(
        identity: BackendIdentity,
        capabilities: CapabilityManifest,
        mode: ExecutionMode,
        handler: RunHandler,
    ) -> Self {
        Self {
            identity,
            capabilities,
            mode,
            handler,
        }
    }

    /// The backend identity of this sidecar.
    #[must_use]
    pub fn identity(&self) -> &BackendIdentity {
        &self.identity
    }

    /// The capability manifest of this sidecar.
    #[must_use]
    pub fn capabilities(&self) -> &CapabilityManifest {
        &self.capabilities
    }

    /// The execution mode of this sidecar.
    #[must_use]
    pub fn execution_mode(&self) -> ExecutionMode {
        self.mode
    }

    /// Run the sidecar protocol loop using the provided reader and writer.
    ///
    /// This is the testable core: it reads envelopes from `reader`, writes
    /// responses to `writer`, and delegates work orders to the run handler.
    ///
    /// # Errors
    ///
    /// Returns [`SidecarError`] on I/O or protocol failures.
    pub async fn run_with_io<R, W>(&self, reader: R, mut writer: W) -> Result<(), SidecarError>
    where
        R: tokio::io::AsyncBufRead + Unpin,
        W: tokio::io::AsyncWrite + Unpin,
    {
        // 1. Send hello
        self.send_hello(&mut writer).await?;

        // 2. Read envelopes from reader
        let mut reader = reader;
        let mut line = String::new();

        loop {
            line.clear();
            let n = reader.read_line(&mut line).await?;
            if n == 0 {
                break; // EOF
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let envelope = JsonlCodec::decode(trimmed)
                .map_err(|e| SidecarError::Protocol(format!("failed to decode envelope: {e}")))?;

            match envelope {
                Envelope::Run { id, work_order } => {
                    self.handle_run(&id, work_order, &mut writer).await?;
                }
                _ => {
                    // Ignore unexpected envelopes (hello, event, etc.)
                    continue;
                }
            }
        }

        Ok(())
    }

    /// Run the sidecar using real stdin/stdout.
    ///
    /// This is the main entry point for a sidecar binary.
    ///
    /// # Errors
    ///
    /// Returns [`SidecarError`] on I/O or protocol failures.
    pub async fn run(self) -> Result<(), SidecarError> {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();
        let reader = BufReader::new(stdin);
        self.run_with_io(reader, stdout).await
    }

    // -- internal ---------------------------------------------------------

    async fn send_hello<W>(&self, writer: &mut W) -> Result<(), SidecarError>
    where
        W: tokio::io::AsyncWrite + Unpin,
    {
        let envelope =
            Envelope::hello_with_mode(self.identity.clone(), self.capabilities.clone(), self.mode);
        let line = JsonlCodec::encode(&envelope)
            .map_err(|e| SidecarError::Protocol(format!("failed to encode hello: {e}")))?;
        writer.write_all(line.as_bytes()).await?;
        writer.flush().await?;
        Ok(())
    }

    async fn handle_run<W>(
        &self,
        run_id: &str,
        work_order: WorkOrder,
        writer: &mut W,
    ) -> Result<(), SidecarError>
    where
        W: tokio::io::AsyncWrite + Unpin,
    {
        let (emitter, mut rx) = EventEmitter::new(run_id, 64);

        let handler = self.handler.clone();

        // Spawn the handler in a background task.
        let handle = tokio::spawn(async move { (handler)(work_order, emitter).await });

        // Stream events from the handler to the writer.
        while let Some(event) = rx.recv().await {
            let envelope = Envelope::Event {
                ref_id: run_id.to_string(),
                event,
            };
            let line = JsonlCodec::encode(&envelope)
                .map_err(|e| SidecarError::Protocol(format!("failed to encode event: {e}")))?;
            writer.write_all(line.as_bytes()).await?;
            writer.flush().await?;
        }

        // Wait for the handler to finish.
        let result = handle
            .await
            .map_err(|e| SidecarError::Handler(format!("handler task panicked: {e}")))?;

        match result {
            Ok(receipt) => {
                let envelope = Envelope::Final {
                    ref_id: run_id.to_string(),
                    receipt,
                };
                let line = JsonlCodec::encode(&envelope)
                    .map_err(|e| SidecarError::Protocol(format!("failed to encode final: {e}")))?;
                writer.write_all(line.as_bytes()).await?;
                writer.flush().await?;
            }
            Err(e) => {
                let envelope = Envelope::Fatal {
                    ref_id: Some(run_id.to_string()),
                    error: e.to_string(),
                    error_code: None,
                };
                let line = JsonlCodec::encode(&envelope)
                    .map_err(|e| SidecarError::Protocol(format!("failed to encode fatal: {e}")))?;
                writer.write_all(line.as_bytes()).await?;
                writer.flush().await?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::{Outcome, ReceiptBuilder};

    #[tokio::test]
    async fn runtime_accessors() {
        let identity = BackendIdentity {
            id: "test".into(),
            backend_version: Some("1.0".into()),
            adapter_version: None,
        };
        let caps = CapabilityManifest::new();
        let handler = std::sync::Arc::new(|_wo: WorkOrder, _em: EventEmitter| {
            Box::pin(async {
                Ok(ReceiptBuilder::new("test")
                    .outcome(Outcome::Complete)
                    .build())
            }) as crate::builder::BoxRunFuture
        });
        let rt = SidecarRuntime::new(identity, caps, ExecutionMode::Mapped, handler);
        assert_eq!(rt.identity().id, "test");
        assert!(rt.capabilities().is_empty());
        assert_eq!(rt.execution_mode(), ExecutionMode::Mapped);
    }
}
