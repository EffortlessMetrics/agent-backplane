// SPDX-License-Identifier: MIT OR Apache-2.0
//! Sidecar client: spawn + handshake + run initiation.

use serde::de::DeserializeOwned;
use serde_json::Value;
use tracing::debug;

use super::{SidecarError, frame::Frame, process::SidecarProcess, run::RawRun, spec::ProcessSpec};

/// Parsed `hello` handshake data from a sidecar (value-based).
#[derive(Debug, Clone)]
pub struct HelloData {
    /// ABP contract version reported by the sidecar.
    pub contract_version: String,
    /// Backend identity as an opaque JSON value.
    pub backend: Value,
    /// Capabilities advertised by the sidecar.
    pub capabilities: Value,
    /// Execution mode (e.g. passthrough or mapped).
    pub mode: Value,
}

impl HelloData {
    /// Deserialize the `backend` value into a concrete type.
    pub fn backend_as<T: DeserializeOwned>(&self) -> Result<T, SidecarError> {
        serde_json::from_value(self.backend.clone()).map_err(SidecarError::Deserialize)
    }

    /// Deserialize the `capabilities` value into a concrete type.
    pub fn capabilities_as<T: DeserializeOwned>(&self) -> Result<T, SidecarError> {
        serde_json::from_value(self.capabilities.clone()).map_err(SidecarError::Deserialize)
    }
}

/// Value-based sidecar client: spawns a process, performs the `hello` handshake,
/// and provides [`run_raw`](SidecarClient::run_raw) to start a run.
pub struct SidecarClient {
    process: SidecarProcess,
    /// Handshake data received from the sidecar.
    pub hello: HelloData,
}

impl SidecarClient {
    /// Spawn a sidecar and wait for its `hello` frame.
    pub async fn spawn(spec: ProcessSpec) -> Result<Self, SidecarError> {
        let mut process = SidecarProcess::spawn(spec).await?;

        // Read the first frame — must be Hello.
        let frame = process.recv().await?;
        let hello = match frame {
            Some(Frame::Hello {
                contract_version,
                backend,
                capabilities,
                mode,
            }) => HelloData {
                contract_version,
                backend,
                capabilities,
                mode,
            },
            Some(other) => {
                return Err(SidecarError::Protocol(format!(
                    "expected hello, got {other:?}"
                )));
            }
            None => {
                return Err(SidecarError::Protocol(
                    "sidecar closed stdout before sending hello".into(),
                ));
            }
        };

        debug!(target: "sidecar_kit", "hello: backend={}", hello.backend);

        Ok(Self { process, hello })
    }

    /// Send a work order and begin a raw (value-based) run.
    pub async fn run_raw(
        mut self,
        run_id: String,
        run_payload: Value,
    ) -> Result<RawRun, SidecarError> {
        // Send the Run frame.
        let run_frame = Frame::Run {
            id: run_id.clone(),
            work_order: run_payload,
        };
        self.process.send(&run_frame).await?;

        RawRun::start(self.process, run_id)
    }
}
