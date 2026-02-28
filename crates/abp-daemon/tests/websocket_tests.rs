// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive WebSocket endpoint tests for the daemon.

use abp_core::{
    CapabilityRequirements, ContextPacket, ExecutionLane, PolicyProfile, RuntimeConfig, WorkOrder,
    WorkspaceMode, WorkspaceSpec,
};
use abp_daemon::{AppState, RunTracker, build_app};
use abp_integrations::MockBackend;
use abp_runtime::Runtime;
use futures::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::RwLock;
use tokio_tungstenite::tungstenite::{self, Message};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_state(receipts_dir: &std::path::Path) -> Arc<AppState> {
    let mut runtime = Runtime::new();
    runtime.register_backend("mock", MockBackend);

    Arc::new(AppState {
        runtime: Arc::new(runtime),
        receipts: Arc::new(RwLock::new(HashMap::new())),
        receipts_dir: receipts_dir.to_path_buf(),
        run_tracker: RunTracker::new(),
    })
}

fn test_work_order() -> WorkOrder {
    WorkOrder {
        id: Uuid::new_v4(),
        task: "ws test task".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
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

/// Spawn the daemon on a random port and return the bound address.
async fn spawn_server(state: Arc<AppState>) -> SocketAddr {
    let app = build_app(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr
}

/// Connect a WebSocket client to the given address.
async fn ws_connect(
    addr: SocketAddr,
) -> (
    futures::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        Message,
    >,
    futures::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
) {
    let url = format!("ws://127.0.0.1:{}/ws", addr.port());
    let (stream, _resp) = tokio_tungstenite::connect_async(&url).await.unwrap();
    stream.split()
}

// ---------------------------------------------------------------------------
// 1. WebSocket connection establishes successfully
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ws_connection_establishes_successfully() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = spawn_server(test_state(tmp.path())).await;

    let url = format!("ws://127.0.0.1:{}/ws", addr.port());
    let (stream, resp) = tokio_tungstenite::connect_async(&url).await.unwrap();
    assert_eq!(
        resp.status(),
        axum::http::StatusCode::SWITCHING_PROTOCOLS
    );
    drop(stream);
}

// ---------------------------------------------------------------------------
// 2. Echo: client sends text and receives it back
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ws_echo_text_message() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = spawn_server(test_state(tmp.path())).await;

    let (mut sink, mut stream) = ws_connect(addr).await;

    sink.send(Message::Text("hello backplane".into()))
        .await
        .unwrap();

    let msg = stream.next().await.unwrap().unwrap();
    match msg {
        Message::Text(text) => assert_eq!(text, "hello backplane"),
        other => panic!("expected Text, got: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 3. Multiple concurrent WebSocket connections
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ws_multiple_concurrent_connections() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = spawn_server(test_state(tmp.path())).await;

    let mut handles = Vec::new();
    for i in 0..5 {
        let a = addr;
        handles.push(tokio::spawn(async move {
            let (mut sink, mut stream) = ws_connect(a).await;
            let payload = format!("conn-{i}");
            sink.send(Message::Text(payload.clone().into()))
                .await
                .unwrap();
            let msg = stream.next().await.unwrap().unwrap();
            match msg {
                Message::Text(text) => assert_eq!(text, payload),
                other => panic!("conn-{i}: expected Text, got: {other:?}"),
            }
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
}

// ---------------------------------------------------------------------------
// 4. WebSocket disconnection handling – server tolerates client drop
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ws_client_disconnect_is_handled_gracefully() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = spawn_server(test_state(tmp.path())).await;

    // Connect and immediately drop (abrupt close).
    {
        let (sink, stream) = ws_connect(addr).await;
        drop(sink);
        drop(stream);
    }

    // The server should still accept new connections after the abrupt close.
    let (mut sink, mut stream) = ws_connect(addr).await;
    sink.send(Message::Text("still alive".into()))
        .await
        .unwrap();
    let msg = stream.next().await.unwrap().unwrap();
    match msg {
        Message::Text(text) => assert_eq!(text, "still alive"),
        other => panic!("expected Text, got: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 5. Invalid WebSocket message handling – binary frames are silently ignored
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ws_binary_message_is_ignored() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = spawn_server(test_state(tmp.path())).await;

    let (mut sink, mut stream) = ws_connect(addr).await;

    // Send a binary frame (the handler ignores non-text, non-close).
    sink.send(Message::Binary(vec![0xDE, 0xAD, 0xBE, 0xEF].into()))
        .await
        .unwrap();

    // Follow up with a text frame to confirm the connection is still alive.
    sink.send(Message::Text("after binary".into()))
        .await
        .unwrap();

    let msg = stream.next().await.unwrap().unwrap();
    match msg {
        Message::Text(text) => assert_eq!(text, "after binary"),
        other => panic!("expected Text echo, got: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 6. Large event payloads over WebSocket
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ws_large_payload() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = spawn_server(test_state(tmp.path())).await;

    let (mut sink, mut stream) = ws_connect(addr).await;

    // 64 KiB text payload.
    let large = "X".repeat(64 * 1024);
    sink.send(Message::Text(large.clone().into()))
        .await
        .unwrap();

    let msg = stream.next().await.unwrap().unwrap();
    match msg {
        Message::Text(text) => assert_eq!(text.len(), large.len()),
        other => panic!("expected Text, got: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 7. Rapid event stream over WebSocket – many messages in quick succession
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ws_rapid_messages() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = spawn_server(test_state(tmp.path())).await;

    let (mut sink, mut stream) = ws_connect(addr).await;

    let count = 100;
    for i in 0..count {
        sink.send(Message::Text(format!("msg-{i}").into()))
            .await
            .unwrap();
    }

    for i in 0..count {
        let msg = stream.next().await.unwrap().unwrap();
        match msg {
            Message::Text(text) => assert_eq!(text, format!("msg-{i}")),
            other => panic!("msg-{i}: expected Text, got: {other:?}"),
        }
    }
}

// ---------------------------------------------------------------------------
// 8. WebSocket and REST API concurrent usage
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ws_and_rest_concurrent() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let addr = spawn_server(state).await;

    // Open a WebSocket connection.
    let (mut sink, mut stream) = ws_connect(addr).await;

    // Concurrently make a raw HTTP request to /health.
    let http_handle = {
        let port = addr.port();
        tokio::spawn(async move {
            let mut tcp =
                tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
                    .await
                    .unwrap();
            tcp.write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n")
                .await
                .unwrap();
            let mut buf = vec![0u8; 4096];
            let n = tcp.read(&mut buf).await.unwrap();
            let response = String::from_utf8_lossy(&buf[..n]);
            assert!(
                response.contains("200 OK"),
                "expected 200 OK in: {response}"
            );
            assert!(
                response.contains("\"status\":\"ok\""),
                "expected status ok in: {response}"
            );
        })
    };

    // Meanwhile, echo over the WebSocket.
    sink.send(Message::Text("during http".into()))
        .await
        .unwrap();
    let msg = stream.next().await.unwrap().unwrap();
    match msg {
        Message::Text(text) => assert_eq!(text, "during http"),
        other => panic!("expected Text, got: {other:?}"),
    }

    http_handle.await.unwrap();
}

// ---------------------------------------------------------------------------
// 9. Connection close frame handling
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ws_close_frame_terminates_session() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = spawn_server(test_state(tmp.path())).await;

    let (mut sink, mut stream) = ws_connect(addr).await;

    // Send a proper Close frame.
    sink.send(Message::Close(Some(tungstenite::protocol::CloseFrame {
        code: tungstenite::protocol::frame::coding::CloseCode::Normal,
        reason: "done".into(),
    })))
    .await
    .unwrap();

    // The next read should yield Close, None, or a protocol reset (the server
    // handler breaks out of the loop on Close without echoing a close frame).
    let msg = stream.next().await;
    match msg {
        Some(Ok(Message::Close(_))) | None => { /* expected */ }
        Some(Err(tungstenite::Error::Protocol(
            tungstenite::error::ProtocolError::ResetWithoutClosingHandshake,
        ))) => { /* server dropped without close handshake – acceptable */ }
        other => panic!("expected Close or stream end, got: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 10. Multiple sequential messages maintain ordering
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ws_message_ordering_preserved() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = spawn_server(test_state(tmp.path())).await;

    let (mut sink, mut stream) = ws_connect(addr).await;

    let messages: Vec<String> = (0..20).map(|i| format!("order-{i:04}")).collect();
    for m in &messages {
        sink.send(Message::Text(m.clone().into())).await.unwrap();
    }

    for expected in &messages {
        let msg = stream.next().await.unwrap().unwrap();
        match msg {
            Message::Text(text) => assert_eq!(&text, expected),
            other => panic!("expected Text({expected}), got: {other:?}"),
        }
    }
}

// ---------------------------------------------------------------------------
// 11. Empty text message echoed correctly
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ws_empty_text_message() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = spawn_server(test_state(tmp.path())).await;

    let (mut sink, mut stream) = ws_connect(addr).await;

    sink.send(Message::Text("".into())).await.unwrap();

    let msg = stream.next().await.unwrap().unwrap();
    match msg {
        Message::Text(text) => assert!(text.is_empty()),
        other => panic!("expected empty Text, got: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 12. JSON payload round-trip over WebSocket
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ws_json_payload_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = spawn_server(test_state(tmp.path())).await;

    let (mut sink, mut stream) = ws_connect(addr).await;

    let payload = serde_json::json!({
        "type": "work_order",
        "task": "test",
        "id": Uuid::new_v4().to_string(),
    });
    let text = serde_json::to_string(&payload).unwrap();
    sink.send(Message::Text(text.clone().into())).await.unwrap();

    let msg = stream.next().await.unwrap().unwrap();
    match msg {
        Message::Text(echoed) => {
            let parsed: serde_json::Value = serde_json::from_str(&echoed).unwrap();
            assert_eq!(parsed, payload);
        }
        other => panic!("expected Text, got: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 13. WebSocket with REST run submission concurrent
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ws_concurrent_with_run_submission() {
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path());
    let addr = spawn_server(state).await;

    // Open a WebSocket connection.
    let (mut sink, mut stream) = ws_connect(addr).await;

    // Submit a run via raw HTTP concurrently.
    let run_handle = {
        let port = addr.port();
        tokio::spawn(async move {
            let req_body = serde_json::json!({
                "backend": "mock",
                "work_order": test_work_order(),
            });
            let body = serde_json::to_string(&req_body).unwrap();
            let raw = format!(
                "POST /run HTTP/1.1\r\n\
                 Host: localhost\r\n\
                 Content-Type: application/json\r\n\
                 Content-Length: {}\r\n\
                 \r\n\
                 {}",
                body.len(),
                body
            );
            let mut tcp =
                tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
                    .await
                    .unwrap();
            tcp.write_all(raw.as_bytes()).await.unwrap();
            let mut buf = vec![0u8; 16384];
            let n = tcp.read(&mut buf).await.unwrap();
            let response = String::from_utf8_lossy(&buf[..n]);
            assert!(
                response.contains("200 OK"),
                "expected 200 OK in run response: {response}"
            );
        })
    };

    // Echo over WebSocket while the run is in-flight.
    sink.send(Message::Text("during run".into()))
        .await
        .unwrap();
    let msg = stream.next().await.unwrap().unwrap();
    match msg {
        Message::Text(text) => assert_eq!(text, "during run"),
        other => panic!("expected Text, got: {other:?}"),
    }

    run_handle.await.unwrap();
}

// ---------------------------------------------------------------------------
// 14. Ping frame handling – server stays alive after ping
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ws_ping_frame_handling() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = spawn_server(test_state(tmp.path())).await;

    let (mut sink, mut stream) = ws_connect(addr).await;

    // Send a Ping frame.
    sink.send(Message::Ping(vec![1, 2, 3].into()))
        .await
        .unwrap();

    // The tungstenite library auto-responds with Pong at the protocol level.
    // Confirm the connection is still usable with a text echo.
    sink.send(Message::Text("after ping".into()))
        .await
        .unwrap();

    // Drain any Pong frames, then expect our text echo.
    loop {
        let msg = stream.next().await.unwrap().unwrap();
        match msg {
            Message::Pong(_) => continue,
            Message::Text(text) => {
                assert_eq!(text, "after ping");
                break;
            }
            other => panic!("unexpected message: {other:?}"),
        }
    }
}

// ---------------------------------------------------------------------------
// 15. Reconnection after close – new connection works
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ws_reconnection_after_close() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = spawn_server(test_state(tmp.path())).await;

    // First connection: send close.
    {
        let (mut sink, _stream) = ws_connect(addr).await;
        sink.send(Message::Close(None)).await.unwrap();
    }

    // Brief pause to let the server process the close.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Second connection should succeed.
    let (mut sink, mut stream) = ws_connect(addr).await;
    sink.send(Message::Text("reconnected".into()))
        .await
        .unwrap();
    let msg = stream.next().await.unwrap().unwrap();
    match msg {
        Message::Text(text) => assert_eq!(text, "reconnected"),
        other => panic!("expected Text, got: {other:?}"),
    }
}
