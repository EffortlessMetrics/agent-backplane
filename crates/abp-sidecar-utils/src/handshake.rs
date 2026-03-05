// SPDX-License-Identifier: MIT OR Apache-2.0
//! Async hello handshake with timeout and contract-version validation.

use std::time::Duration;

use abp_core::{BackendIdentity, CapabilityManifest, ExecutionMode, CONTRACT_VERSION};
use abp_protocol::{is_compatible_version, Envelope, JsonlCodec, ProtocolError};
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

/// Default handshake timeout (10 seconds).
pub const DEFAULT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// Information extracted from a successful hello handshake.
#[derive(Debug, Clone)]
pub struct HelloInfo {
    /// Contract version reported by the peer.
    pub contract_version: String,
    /// Backend identity of the peer.
    pub backend: BackendIdentity,
    /// Capabilities advertised by the peer.
    pub capabilities: CapabilityManifest,
    /// Execution mode the peer will use.
    pub mode: ExecutionMode,
}

/// Errors specific to the handshake phase.
#[derive(Debug, Error)]
pub enum HandshakeError {
    /// The hello was not received within the timeout.
    #[error("handshake timed out after {0:?}")]
    Timeout(Duration),
    /// The peer sent an incompatible contract version.
    #[error(
        "incompatible contract version: got \"{got}\", expected compatible with \"{expected}\""
    )]
    IncompatibleVersion {
        /// Version the peer advertised.
        got: String,
        /// Our local version.
        expected: String,
    },
    /// The peer sent a message that was not a hello.
    #[error("expected hello, got: {0}")]
    UnexpectedMessage(String),
    /// I/O error during the handshake.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// Protocol-level decoding error.
    #[error("protocol error: {0}")]
    Protocol(#[from] ProtocolError),
    /// The peer closed the connection before sending hello.
    #[error("peer closed connection before hello")]
    PeerClosed,
}

/// Manages the sidecar hello handshake.
///
/// # Examples
///
/// ```no_run
/// use abp_sidecar_utils::handshake::HandshakeManager;
/// use std::time::Duration;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let (reader, _writer) = tokio::io::duplex(1024);
/// let reader = tokio::io::BufReader::new(reader);
/// let info = HandshakeManager::await_hello(reader, Duration::from_secs(5)).await?;
/// println!("Peer: {}", info.backend.id);
/// # Ok(())
/// # }
/// ```
pub struct HandshakeManager;

impl HandshakeManager {
    /// Wait for a hello envelope from the peer, with a timeout.
    ///
    /// Validates that the contract version is compatible with ours.
    ///
    /// # Errors
    ///
    /// Returns [`HandshakeError`] on timeout, incompatible version, I/O
    /// failure, or if the first message is not a hello.
    pub async fn await_hello<R>(reader: R, timeout: Duration) -> Result<HelloInfo, HandshakeError>
    where
        R: tokio::io::AsyncBufRead + Unpin,
    {
        let mut reader = reader;
        let mut line = String::new();

        let read_result = tokio::time::timeout(timeout, reader.read_line(&mut line)).await;

        let n = match read_result {
            Ok(Ok(n)) => n,
            Ok(Err(e)) => return Err(HandshakeError::Io(e)),
            Err(_) => return Err(HandshakeError::Timeout(timeout)),
        };

        if n == 0 {
            return Err(HandshakeError::PeerClosed);
        }

        let envelope = JsonlCodec::decode(line.trim_end())?;

        match envelope {
            Envelope::Hello {
                contract_version,
                backend,
                capabilities,
                mode,
            } => {
                if !is_compatible_version(&contract_version, CONTRACT_VERSION) {
                    return Err(HandshakeError::IncompatibleVersion {
                        got: contract_version,
                        expected: CONTRACT_VERSION.to_string(),
                    });
                }
                Ok(HelloInfo {
                    contract_version,
                    backend,
                    capabilities,
                    mode,
                })
            }
            other => Err(HandshakeError::UnexpectedMessage(format!("{other:?}"))),
        }
    }

    /// Send a hello envelope to the peer.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if writing fails.
    pub async fn send_hello<W>(
        mut writer: W,
        backend: BackendIdentity,
        capabilities: CapabilityManifest,
    ) -> Result<(), HandshakeError>
    where
        W: tokio::io::AsyncWrite + Unpin,
    {
        let envelope = Envelope::hello(backend, capabilities);
        let line = JsonlCodec::encode(&envelope).map_err(HandshakeError::Protocol)?;
        writer
            .write_all(line.as_bytes())
            .await
            .map_err(HandshakeError::Io)?;
        writer.flush().await.map_err(HandshakeError::Io)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::{BackendIdentity, CapabilityManifest};
    use tokio::io::BufReader;

    fn make_hello_line(version: &str) -> String {
        let env = Envelope::Hello {
            contract_version: version.to_string(),
            backend: BackendIdentity {
                id: "test-sidecar".into(),
                backend_version: Some("1.0".into()),
                adapter_version: None,
            },
            capabilities: CapabilityManifest::new(),
            mode: ExecutionMode::default(),
        };
        JsonlCodec::encode(&env).unwrap()
    }

    #[tokio::test]
    async fn await_hello_success() {
        let hello = make_hello_line(CONTRACT_VERSION);
        let reader = BufReader::new(hello.as_bytes());
        let info = HandshakeManager::await_hello(reader, DEFAULT_HANDSHAKE_TIMEOUT)
            .await
            .unwrap();
        assert_eq!(info.backend.id, "test-sidecar");
        assert_eq!(info.contract_version, CONTRACT_VERSION);
    }

    #[tokio::test]
    async fn await_hello_incompatible_version() {
        let hello = make_hello_line("abp/v99.0");
        let reader = BufReader::new(hello.as_bytes());
        let err = HandshakeManager::await_hello(reader, DEFAULT_HANDSHAKE_TIMEOUT)
            .await
            .unwrap_err();
        assert!(matches!(err, HandshakeError::IncompatibleVersion { .. }));
    }

    #[tokio::test]
    async fn await_hello_unexpected_message() {
        let fatal = "{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"nope\"}\n";
        let reader = BufReader::new(fatal.as_bytes());
        let err = HandshakeManager::await_hello(reader, DEFAULT_HANDSHAKE_TIMEOUT)
            .await
            .unwrap_err();
        assert!(matches!(err, HandshakeError::UnexpectedMessage(_)));
    }

    #[tokio::test]
    async fn await_hello_peer_closed() {
        let reader = BufReader::new(&b""[..]);
        let err = HandshakeManager::await_hello(reader, DEFAULT_HANDSHAKE_TIMEOUT)
            .await
            .unwrap_err();
        assert!(matches!(err, HandshakeError::PeerClosed));
    }

    #[tokio::test]
    async fn await_hello_timeout() {
        // A reader that never produces data.
        let (reader, _writer) = tokio::io::duplex(64);
        let reader = BufReader::new(reader);
        let err = HandshakeManager::await_hello(reader, Duration::from_millis(50))
            .await
            .unwrap_err();
        assert!(matches!(err, HandshakeError::Timeout(_)));
    }

    #[tokio::test]
    async fn send_hello_roundtrip() {
        let mut buf = Vec::new();
        let backend = BackendIdentity {
            id: "roundtrip".into(),
            backend_version: None,
            adapter_version: None,
        };
        HandshakeManager::send_hello(&mut buf, backend, CapabilityManifest::new())
            .await
            .unwrap();

        let reader = BufReader::new(buf.as_slice());
        let info = HandshakeManager::await_hello(reader, DEFAULT_HANDSHAKE_TIMEOUT)
            .await
            .unwrap();
        assert_eq!(info.backend.id, "roundtrip");
    }
}
