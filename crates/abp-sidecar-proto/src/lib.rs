// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
//! abp-sidecar-proto
#![deny(unsafe_code)]
#![warn(missing_docs)]
//!
//! Sidecar-side helpers for the ABP JSONL protocol.
//!
//! This crate is the counterpart to `abp-host`. While `abp-host` manages
//! sidecar processes from the control plane, this crate provides utilities
//! for *implementing* a sidecar that speaks the JSONL protocol over
//! stdin/stdout.

use abp_core::{AgentEvent, BackendIdentity, CapabilityManifest, Receipt, WorkOrder};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};
use async_trait::async_trait;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors from sidecar protocol operations.
#[derive(Debug, Error)]
pub enum SidecarProtoError {
    /// JSON serialization or deserialization failure.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    /// Underlying I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSONL protocol-level error.
    #[error("protocol error: {0}")]
    Protocol(#[from] ProtocolError),

    /// Received an unexpected envelope at this point in the protocol.
    #[error("unexpected message: expected {expected}, got {got}")]
    UnexpectedMessage {
        /// The envelope type that was expected.
        expected: String,
        /// The envelope type that was actually received.
        got: String,
    },

    /// The handler reported an application-level error.
    #[error("handler error: {0}")]
    Handler(String),

    /// Stdin closed before receiving a run envelope.
    #[error("stdin closed unexpectedly")]
    StdinClosed,

    /// The internal event channel was closed unexpectedly.
    #[error("event channel closed")]
    ChannelClosed,
}

// ---------------------------------------------------------------------------
// SidecarHandler trait
// ---------------------------------------------------------------------------

/// Trait implemented by sidecar authors to handle incoming work orders.
///
/// The [`SidecarServer`] dispatches parsed protocol messages to these methods.
/// Return `Ok(())` after calling [`EventSender::send_final`] to indicate
/// success, or return `Err` to have the server emit a `fatal` envelope.
#[async_trait]
pub trait SidecarHandler: Send + Sync + 'static {
    /// Called when the control plane sends a `run` envelope with a work order.
    ///
    /// Use the [`EventSender`] to stream events and send the final receipt.
    /// Return `Err` to have the server send a `fatal` envelope automatically.
    async fn on_run(
        &self,
        run_id: String,
        work_order: WorkOrder,
        sender: EventSender,
    ) -> Result<(), SidecarProtoError>;

    /// Called when the control plane requests cancellation.
    ///
    /// The default implementation does nothing.
    async fn on_cancel(&self) -> Result<(), SidecarProtoError> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// EventSender — channel-based utility for streaming envelopes
// ---------------------------------------------------------------------------

/// Handle for sending [`AgentEvent`]s back to the control plane.
///
/// Uses an internal channel so that the [`SidecarServer`] can drain
/// envelopes to the actual writer. Cheaply cloneable.
#[derive(Clone, Debug)]
pub struct EventSender {
    tx: mpsc::UnboundedSender<Envelope>,
    ref_id: String,
}

impl EventSender {
    /// Create a new sender backed by the given channel.
    pub fn new(tx: mpsc::UnboundedSender<Envelope>, ref_id: impl Into<String>) -> Self {
        Self {
            tx,
            ref_id: ref_id.into(),
        }
    }

    /// The run id this sender is bound to.
    #[must_use]
    pub fn ref_id(&self) -> &str {
        &self.ref_id
    }

    /// Send a single [`AgentEvent`] as an `event` envelope.
    pub async fn send_event(&self, event: AgentEvent) -> Result<(), SidecarProtoError> {
        let envelope = Envelope::Event {
            ref_id: self.ref_id.clone(),
            event,
        };
        self.tx
            .send(envelope)
            .map_err(|_| SidecarProtoError::ChannelClosed)
    }

    /// Send the final [`Receipt`] as a `final` envelope.
    pub async fn send_final(&self, receipt: Receipt) -> Result<(), SidecarProtoError> {
        let envelope = Envelope::Final {
            ref_id: self.ref_id.clone(),
            receipt,
        };
        self.tx
            .send(envelope)
            .map_err(|_| SidecarProtoError::ChannelClosed)
    }

    /// Send a fatal error envelope.
    pub async fn send_fatal(&self, error: impl Into<String>) -> Result<(), SidecarProtoError> {
        let envelope = Envelope::Fatal {
            ref_id: Some(self.ref_id.clone()),
            error: error.into(),
            error_code: None,
        };
        self.tx
            .send(envelope)
            .map_err(|_| SidecarProtoError::ChannelClosed)
    }
}

// ---------------------------------------------------------------------------
// Free-standing helper functions (write directly to an AsyncWrite)
// ---------------------------------------------------------------------------

/// Write a `hello` envelope to `writer`.
///
/// This should be the first line a sidecar emits on stdout.
pub async fn send_hello(
    writer: &mut (impl tokio::io::AsyncWrite + Unpin),
    identity: BackendIdentity,
    capabilities: CapabilityManifest,
) -> Result<(), SidecarProtoError> {
    let envelope = Envelope::hello(identity, capabilities);
    write_envelope(writer, &envelope).await
}

/// Write an `event` envelope to `writer`.
pub async fn send_event(
    writer: &mut (impl tokio::io::AsyncWrite + Unpin),
    ref_id: &str,
    event: AgentEvent,
) -> Result<(), SidecarProtoError> {
    let envelope = Envelope::Event {
        ref_id: ref_id.to_string(),
        event,
    };
    write_envelope(writer, &envelope).await
}

/// Write a `final` envelope to `writer`.
pub async fn send_final(
    writer: &mut (impl tokio::io::AsyncWrite + Unpin),
    ref_id: &str,
    receipt: Receipt,
) -> Result<(), SidecarProtoError> {
    let envelope = Envelope::Final {
        ref_id: ref_id.to_string(),
        receipt,
    };
    write_envelope(writer, &envelope).await
}

/// Write a `fatal` envelope to `writer`.
pub async fn send_fatal(
    writer: &mut (impl tokio::io::AsyncWrite + Unpin),
    ref_id: Option<String>,
    error: impl Into<String>,
) -> Result<(), SidecarProtoError> {
    let envelope = Envelope::Fatal {
        ref_id,
        error: error.into(),
        error_code: None,
    };
    write_envelope(writer, &envelope).await
}

async fn write_envelope(
    writer: &mut (impl tokio::io::AsyncWrite + Unpin),
    envelope: &Envelope,
) -> Result<(), SidecarProtoError> {
    let line = JsonlCodec::encode(envelope)?;
    writer.write_all(line.as_bytes()).await?;
    writer.flush().await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// SidecarServer
// ---------------------------------------------------------------------------

/// A server that reads JSONL from stdin and dispatches to a [`SidecarHandler`].
///
/// This is the main entry point for sidecar implementations. Typical usage:
///
/// ```no_run
/// use abp_sidecar_proto::{SidecarServer, SidecarHandler, EventSender, SidecarProtoError};
/// use abp_core::{WorkOrder, BackendIdentity, CapabilityManifest};
/// use async_trait::async_trait;
///
/// struct MyHandler;
///
/// #[async_trait]
/// impl SidecarHandler for MyHandler {
///     async fn on_run(
///         &self,
///         _run_id: String,
///         _wo: WorkOrder,
///         sender: EventSender,
///     ) -> Result<(), SidecarProtoError> {
///         // ... process work order, stream events via sender ...
///         Ok(())
///     }
/// }
///
/// #[tokio::main]
/// async fn main() {
///     let identity = BackendIdentity {
///         id: "my-sidecar".into(),
///         backend_version: Some("1.0".into()),
///         adapter_version: None,
///     };
///     let server = SidecarServer::new(MyHandler, identity, CapabilityManifest::new());
///     server.run().await.unwrap();
/// }
/// ```
pub struct SidecarServer<H> {
    handler: H,
    identity: BackendIdentity,
    capabilities: CapabilityManifest,
}

impl<H: SidecarHandler> SidecarServer<H> {
    /// Create a new server with the given handler and backend identity.
    pub fn new(handler: H, identity: BackendIdentity, capabilities: CapabilityManifest) -> Self {
        Self {
            handler,
            identity,
            capabilities,
        }
    }

    /// Run the sidecar protocol loop over real stdin/stdout.
    ///
    /// 1. Sends the `hello` envelope.
    /// 2. Reads a `run` envelope from stdin.
    /// 3. Dispatches to the handler.
    /// 4. Sends buffered envelopes and, on error, a `fatal` envelope.
    pub async fn run(self) -> Result<(), SidecarProtoError> {
        let stdin = tokio::io::stdin();
        let mut stdout = tokio::io::stdout();
        self.run_with_io(stdin, &mut stdout).await
    }

    /// Run the protocol loop with injectable I/O (for testing).
    pub async fn run_with_io<R, W>(self, reader: R, writer: &mut W) -> Result<(), SidecarProtoError>
    where
        R: tokio::io::AsyncRead + Send + Unpin,
        W: tokio::io::AsyncWrite + Send + Unpin,
    {
        let Self {
            handler,
            identity,
            capabilities,
        } = self;

        // Step 1: send hello
        send_hello(writer, identity, capabilities).await?;

        // Step 2: read envelopes from stdin until we get a Run
        let mut reader = BufReader::new(reader);
        let mut line = String::new();

        let (run_id, work_order) = loop {
            line.clear();
            let n = reader.read_line(&mut line).await?;
            if n == 0 {
                return Err(SidecarProtoError::StdinClosed);
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let envelope = JsonlCodec::decode(trimmed)?;
            match envelope {
                Envelope::Run { id, work_order } => break (id, work_order),
                other => {
                    return Err(SidecarProtoError::UnexpectedMessage {
                        expected: "run".into(),
                        got: format!("{:?}", std::mem::discriminant(&other)),
                    });
                }
            }
        };

        // Step 3: create channel-based EventSender and dispatch to handler
        let (tx, mut rx) = mpsc::unbounded_channel::<Envelope>();
        let sender = EventSender::new(tx, run_id.clone());

        let handler_result = handler.on_run(run_id.clone(), work_order, sender).await;

        // Step 4: drain buffered envelopes to writer
        while let Ok(envelope) = rx.try_recv() {
            write_envelope(writer, &envelope).await?;
        }

        // Step 5: on error, send a fatal envelope
        if let Err(e) = handler_result {
            let fatal = Envelope::Fatal {
                ref_id: Some(run_id),
                error: e.to_string(),
                error_code: None,
            };
            write_envelope(writer, &fatal).await?;
        }

        Ok(())
    }
}

impl<H> std::fmt::Debug for SidecarServer<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SidecarServer")
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::*;
    use chrono::Utc;
    use tokio::io::AsyncReadExt;
    use uuid::Uuid;

    fn test_identity() -> BackendIdentity {
        BackendIdentity {
            id: "test-sidecar".into(),
            backend_version: Some("0.1.0".into()),
            adapter_version: None,
        }
    }

    fn test_capabilities() -> CapabilityManifest {
        let mut m = CapabilityManifest::new();
        m.insert(Capability::Streaming, SupportLevel::Native);
        m
    }

    fn test_work_order() -> WorkOrder {
        WorkOrder {
            id: Uuid::nil(),
            task: "hello world".into(),
            lane: ExecutionLane::PatchFirst,
            workspace: WorkspaceSpec {
                root: "/tmp/test".into(),
                mode: WorkspaceMode::PassThrough,
                include: vec![],
                exclude: vec![],
            },
            context: ContextPacket::default(),
            policy: PolicyProfile::default(),
            requirements: CapabilityRequirements::default(),
            config: RuntimeConfig::default(),
        }
    }

    fn test_receipt(run_id: Uuid) -> Receipt {
        Receipt {
            meta: RunMetadata {
                run_id,
                work_order_id: Uuid::nil(),
                contract_version: CONTRACT_VERSION.into(),
                started_at: Utc::now(),
                finished_at: Utc::now(),
                duration_ms: 42,
            },
            backend: test_identity(),
            capabilities: test_capabilities(),
            mode: ExecutionMode::default(),
            usage_raw: serde_json::Value::Null,
            usage: UsageNormalized::default(),
            trace: vec![],
            artifacts: vec![],
            verification: VerificationReport::default(),
            outcome: Outcome::Complete,
            receipt_sha256: None,
        }
    }

    fn test_event() -> AgentEvent {
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "started".into(),
            },
            ext: None,
        }
    }

    /// Read all bytes from the read-half of a duplex, returning a String.
    async fn drain_duplex(mut r: tokio::io::DuplexStream) -> String {
        let mut buf = Vec::new();
        r.read_to_end(&mut buf).await.unwrap();
        String::from_utf8(buf).unwrap()
    }

    fn build_run_input(run_id: &str, wo: &WorkOrder) -> Vec<u8> {
        let env = Envelope::Run {
            id: run_id.into(),
            work_order: wo.clone(),
        };
        JsonlCodec::encode(&env).unwrap().into_bytes()
    }

    // -- Hello envelope tests -----------------------------------------------

    #[tokio::test]
    async fn hello_envelope_serialization() {
        let (mut w, r) = tokio::io::duplex(4096);
        send_hello(&mut w, test_identity(), test_capabilities())
            .await
            .unwrap();
        drop(w);
        let line = drain_duplex(r).await;
        assert!(line.contains("\"t\":\"hello\""));
        assert!(line.contains("\"test-sidecar\""));
        assert!(line.ends_with('\n'));
    }

    #[tokio::test]
    async fn hello_roundtrip() {
        let (mut w, r) = tokio::io::duplex(4096);
        send_hello(&mut w, test_identity(), test_capabilities())
            .await
            .unwrap();
        drop(w);
        let line = drain_duplex(r).await;
        let env = JsonlCodec::decode(line.trim()).unwrap();
        match env {
            Envelope::Hello {
                backend,
                contract_version,
                ..
            } => {
                assert_eq!(backend.id, "test-sidecar");
                assert_eq!(contract_version, CONTRACT_VERSION);
            }
            other => panic!("expected Hello, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn hello_contains_contract_version() {
        let (mut w, r) = tokio::io::duplex(4096);
        send_hello(&mut w, test_identity(), CapabilityManifest::new())
            .await
            .unwrap();
        drop(w);
        let line = drain_duplex(r).await;
        assert!(line.contains(CONTRACT_VERSION));
    }

    #[tokio::test]
    async fn hello_contains_capabilities() {
        let (mut w, r) = tokio::io::duplex(4096);
        send_hello(&mut w, test_identity(), test_capabilities())
            .await
            .unwrap();
        drop(w);
        let line = drain_duplex(r).await;
        assert!(line.contains("streaming"));
    }

    // -- Event sending tests ------------------------------------------------

    #[tokio::test]
    async fn send_event_serialization() {
        let (mut w, r) = tokio::io::duplex(4096);
        send_event(&mut w, "run-1", test_event()).await.unwrap();
        drop(w);
        let line = drain_duplex(r).await;
        assert!(line.contains("\"t\":\"event\""));
        assert!(line.contains("\"ref_id\":\"run-1\""));
    }

    #[tokio::test]
    async fn send_event_roundtrip() {
        let (mut w, r) = tokio::io::duplex(4096);
        send_event(&mut w, "run-1", test_event()).await.unwrap();
        drop(w);
        let line = drain_duplex(r).await;
        let env = JsonlCodec::decode(line.trim()).unwrap();
        match env {
            Envelope::Event { ref_id, event } => {
                assert_eq!(ref_id, "run-1");
                assert!(matches!(event.kind, AgentEventKind::RunStarted { .. }));
            }
            other => panic!("expected Event, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn event_sender_send_event() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let sender = EventSender::new(tx, "run-42");
        assert_eq!(sender.ref_id(), "run-42");
        sender.send_event(test_event()).await.unwrap();
        let envelope = rx.try_recv().unwrap();
        assert!(matches!(envelope, Envelope::Event { .. }));
    }

    #[tokio::test]
    async fn event_sender_clone_shares_channel() {
        let (tx, rx) = mpsc::unbounded_channel();
        let sender = EventSender::new(tx, "run-99");
        let clone = sender.clone();
        assert_eq!(clone.ref_id(), "run-99");
        sender.send_event(test_event()).await.unwrap();
        clone.send_event(test_event()).await.unwrap();
        assert_eq!(rx.len(), 2);
    }

    // -- Receipt finalization tests -----------------------------------------

    #[tokio::test]
    async fn send_final_serialization() {
        let (mut w, r) = tokio::io::duplex(8192);
        let receipt = test_receipt(Uuid::nil());
        send_final(&mut w, "run-1", receipt).await.unwrap();
        drop(w);
        let line = drain_duplex(r).await;
        assert!(line.contains("\"t\":\"final\""));
        assert!(line.contains("\"ref_id\":\"run-1\""));
    }

    #[tokio::test]
    async fn send_final_roundtrip() {
        let (mut w, r) = tokio::io::duplex(8192);
        let receipt = test_receipt(Uuid::nil());
        send_final(&mut w, "run-1", receipt).await.unwrap();
        drop(w);
        let line = drain_duplex(r).await;
        let env = JsonlCodec::decode(line.trim()).unwrap();
        match env {
            Envelope::Final { ref_id, receipt } => {
                assert_eq!(ref_id, "run-1");
                assert_eq!(receipt.outcome, Outcome::Complete);
            }
            other => panic!("expected Final, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn event_sender_send_final() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let sender = EventSender::new(tx, "run-fin");
        let receipt = test_receipt(Uuid::nil());
        sender.send_final(receipt).await.unwrap();
        let envelope = rx.try_recv().unwrap();
        assert!(matches!(envelope, Envelope::Final { .. }));
    }

    // -- Fatal error tests --------------------------------------------------

    #[tokio::test]
    async fn send_fatal_serialization() {
        let (mut w, r) = tokio::io::duplex(4096);
        send_fatal(&mut w, Some("run-1".into()), "boom")
            .await
            .unwrap();
        drop(w);
        let line = drain_duplex(r).await;
        assert!(line.contains("\"t\":\"fatal\""));
        assert!(line.contains("\"error\":\"boom\""));
    }

    #[tokio::test]
    async fn send_fatal_without_ref_id() {
        let (mut w, r) = tokio::io::duplex(4096);
        send_fatal(&mut w, None, "early failure").await.unwrap();
        drop(w);
        let line = drain_duplex(r).await;
        assert!(line.contains("\"ref_id\":null"));
    }

    #[tokio::test]
    async fn send_fatal_roundtrip() {
        let (mut w, r) = tokio::io::duplex(4096);
        send_fatal(&mut w, Some("run-1".into()), "crash")
            .await
            .unwrap();
        drop(w);
        let line = drain_duplex(r).await;
        let env = JsonlCodec::decode(line.trim()).unwrap();
        match env {
            Envelope::Fatal { ref_id, error, .. } => {
                assert_eq!(ref_id, Some("run-1".into()));
                assert_eq!(error, "crash");
            }
            other => panic!("expected Fatal, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn event_sender_send_fatal() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let sender = EventSender::new(tx, "run-err");
        sender.send_fatal("handler exploded").await.unwrap();
        let envelope = rx.try_recv().unwrap();
        assert!(matches!(envelope, Envelope::Fatal { .. }));
    }

    // -- Handler trait dispatch tests ---------------------------------------

    struct EchoHandler;

    #[async_trait]
    impl SidecarHandler for EchoHandler {
        async fn on_run(
            &self,
            _run_id: String,
            work_order: WorkOrder,
            sender: EventSender,
        ) -> Result<(), SidecarProtoError> {
            sender
                .send_event(AgentEvent {
                    ts: Utc::now(),
                    kind: AgentEventKind::AssistantMessage {
                        text: work_order.task.clone(),
                    },
                    ext: None,
                })
                .await?;
            sender.send_final(test_receipt(work_order.id)).await?;
            Ok(())
        }
    }

    struct FailingHandler;

    #[async_trait]
    impl SidecarHandler for FailingHandler {
        async fn on_run(
            &self,
            _run_id: String,
            _work_order: WorkOrder,
            _sender: EventSender,
        ) -> Result<(), SidecarProtoError> {
            Err(SidecarProtoError::Handler("intentional failure".into()))
        }
    }

    #[tokio::test]
    async fn server_echo_handler_full_sequence() {
        let wo = test_work_order();
        let input = build_run_input("run-echo", &wo);
        let (mut w, r) = tokio::io::duplex(16384);

        let server = SidecarServer::new(EchoHandler, test_identity(), test_capabilities());
        server.run_with_io(input.as_slice(), &mut w).await.unwrap();
        drop(w);

        let text = drain_duplex(r).await;
        let lines: Vec<&str> = text.lines().collect();

        // First line: hello
        let hello = JsonlCodec::decode(lines[0]).unwrap();
        assert!(matches!(hello, Envelope::Hello { .. }));

        // Second line: event (the echoed message)
        let event = JsonlCodec::decode(lines[1]).unwrap();
        assert!(matches!(event, Envelope::Event { .. }));

        // Third line: final receipt
        let fin = JsonlCodec::decode(lines[2]).unwrap();
        assert!(matches!(fin, Envelope::Final { .. }));
    }

    #[tokio::test]
    async fn server_failing_handler_sends_fatal() {
        let wo = test_work_order();
        let input = build_run_input("run-fail", &wo);
        let (mut w, r) = tokio::io::duplex(16384);

        let server = SidecarServer::new(FailingHandler, test_identity(), test_capabilities());
        server.run_with_io(input.as_slice(), &mut w).await.unwrap();
        drop(w);

        let text = drain_duplex(r).await;
        let lines: Vec<&str> = text.lines().collect();

        // First line: hello
        assert!(lines[0].contains("\"t\":\"hello\""));

        // Second line: fatal
        let fatal = JsonlCodec::decode(lines[1]).unwrap();
        match fatal {
            Envelope::Fatal { error, .. } => {
                assert!(error.contains("intentional failure"));
            }
            other => panic!("expected Fatal, got {other:?}"),
        }
    }

    // -- Protocol sequence validation tests ---------------------------------

    #[tokio::test]
    async fn server_rejects_non_run_envelope() {
        let env = Envelope::hello(test_identity(), test_capabilities());
        let input = JsonlCodec::encode(&env).unwrap().into_bytes();
        let (mut w, _r) = tokio::io::duplex(4096);

        let server = SidecarServer::new(EchoHandler, test_identity(), test_capabilities());
        let result = server.run_with_io(input.as_slice(), &mut w).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("unexpected message"));
    }

    #[tokio::test]
    async fn server_handles_empty_stdin() {
        let input: &[u8] = b"";
        let (mut w, _r) = tokio::io::duplex(4096);

        let server = SidecarServer::new(EchoHandler, test_identity(), test_capabilities());
        let result = server.run_with_io(input, &mut w).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SidecarProtoError::StdinClosed
        ));
    }

    #[tokio::test]
    async fn server_skips_blank_lines_before_run() {
        let wo = test_work_order();
        let run_line = JsonlCodec::encode(&Envelope::Run {
            id: "run-blank".into(),
            work_order: wo,
        })
        .unwrap();

        let mut input_str = String::new();
        input_str.push('\n');
        input_str.push_str("   \n");
        input_str.push_str(&run_line);

        let (mut w, r) = tokio::io::duplex(16384);
        let server = SidecarServer::new(EchoHandler, test_identity(), test_capabilities());
        server
            .run_with_io(input_str.as_bytes(), &mut w)
            .await
            .unwrap();
        drop(w);

        let text = drain_duplex(r).await;
        assert!(text.contains("\"t\":\"hello\""));
        assert!(text.contains("\"t\":\"final\""));
    }

    #[tokio::test]
    async fn server_invalid_json_returns_error() {
        let input = b"this is not json\n";
        let (mut w, _r) = tokio::io::duplex(4096);

        let server = SidecarServer::new(EchoHandler, test_identity(), test_capabilities());
        let result = server.run_with_io(input.as_slice(), &mut w).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn default_on_cancel_succeeds() {
        let handler = EchoHandler;
        handler.on_cancel().await.unwrap();
    }

    #[tokio::test]
    async fn error_display_messages() {
        let e = SidecarProtoError::Handler("test error".into());
        assert_eq!(e.to_string(), "handler error: test error");

        let e = SidecarProtoError::StdinClosed;
        assert_eq!(e.to_string(), "stdin closed unexpectedly");

        let e = SidecarProtoError::UnexpectedMessage {
            expected: "run".into(),
            got: "hello".into(),
        };
        assert!(e.to_string().contains("unexpected message"));

        let e = SidecarProtoError::ChannelClosed;
        assert_eq!(e.to_string(), "event channel closed");
    }

    #[tokio::test]
    async fn event_sender_debug_impl() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let sender = EventSender::new(tx, "run-dbg");
        let debug = format!("{sender:?}");
        assert!(debug.contains("run-dbg"));
        assert!(debug.contains("EventSender"));
    }

    #[tokio::test]
    async fn multiple_events_streaming() {
        let (mut w, r) = tokio::io::duplex(8192);

        for i in 0..5 {
            let event = AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantDelta {
                    text: format!("token-{i}"),
                },
                ext: None,
            };
            send_event(&mut w, "run-multi", event).await.unwrap();
        }
        drop(w);

        let text = drain_duplex(r).await;
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 5);
        for (i, line) in lines.iter().enumerate() {
            assert!(line.contains(&format!("token-{i}")));
        }
    }

    #[tokio::test]
    async fn full_protocol_sequence_hello_event_final() {
        let (mut w, r) = tokio::io::duplex(16384);

        send_hello(&mut w, test_identity(), test_capabilities())
            .await
            .unwrap();
        send_event(&mut w, "run-seq", test_event()).await.unwrap();
        send_final(&mut w, "run-seq", test_receipt(Uuid::nil()))
            .await
            .unwrap();
        drop(w);

        let text = drain_duplex(r).await;
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3);

        let hello = JsonlCodec::decode(lines[0]).unwrap();
        assert!(matches!(hello, Envelope::Hello { .. }));

        let event = JsonlCodec::decode(lines[1]).unwrap();
        assert!(matches!(event, Envelope::Event { .. }));

        let fin = JsonlCodec::decode(lines[2]).unwrap();
        assert!(matches!(fin, Envelope::Final { .. }));
    }

    #[tokio::test]
    async fn event_sender_closed_channel_error() {
        let (tx, rx) = mpsc::unbounded_channel();
        let sender = EventSender::new(tx, "run-closed");
        drop(rx);
        let result = sender.send_event(test_event()).await;
        assert!(matches!(result, Err(SidecarProtoError::ChannelClosed)));
    }
}
