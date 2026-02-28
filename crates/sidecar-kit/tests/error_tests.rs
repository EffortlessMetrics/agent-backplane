// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive error-path tests for sidecar-kit.

use serde_json::{Value, json};
use sidecar_kit::{Frame, JsonlCodec, SidecarError};
use std::error::Error as StdError;

// ── Source chain ─────────────────────────────────────────────────────

#[test]
fn serialize_error_preserves_source() {
    let serde_err = serde_json::from_str::<()>("not json").unwrap_err();
    let e = SidecarError::Serialize(serde_err);
    assert!(e.source().is_some(), "Serialize should expose source");
}

#[test]
fn deserialize_error_preserves_source() {
    let serde_err = serde_json::from_str::<()>("not json").unwrap_err();
    let e = SidecarError::Deserialize(serde_err);
    assert!(e.source().is_some(), "Deserialize should expose source");
}

#[test]
fn spawn_error_preserves_source() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "no such file");
    let e = SidecarError::Spawn(io_err);
    let src = e.source().expect("Spawn should expose source");
    assert!(src.to_string().contains("no such file"));
}

#[test]
fn stdout_error_preserves_source() {
    let io_err = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broke");
    let e = SidecarError::Stdout(io_err);
    let src = e.source().expect("Stdout should expose source");
    assert!(src.to_string().contains("pipe broke"));
}

#[test]
fn stdin_error_preserves_source() {
    let io_err = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broke");
    let e = SidecarError::Stdin(io_err);
    let src = e.source().expect("Stdin should expose source");
    assert!(src.to_string().contains("pipe broke"));
}

#[test]
fn protocol_error_has_no_source() {
    let e = SidecarError::Protocol("bad handshake".into());
    assert!(e.source().is_none());
}

#[test]
fn fatal_error_has_no_source() {
    let e = SidecarError::Fatal("crash".into());
    assert!(e.source().is_none());
}

#[test]
fn exited_error_has_no_source() {
    let e = SidecarError::Exited(Some(1));
    assert!(e.source().is_none());
}

#[test]
fn timeout_error_has_no_source() {
    let e = SidecarError::Timeout;
    assert!(e.source().is_none());
}

// ── Send + Sync ──────────────────────────────────────────────────────

fn _assert_send<T: Send>() {}
fn _assert_sync<T: Sync>() {}

#[test]
fn error_is_send_and_sync() {
    _assert_send::<SidecarError>();
    _assert_sync::<SidecarError>();
}

// ── Codec error paths ────────────────────────────────────────────────

#[test]
fn codec_decode_invalid_json_gives_deserialize_variant() {
    let err = JsonlCodec::decode("{{{{not json").unwrap_err();
    assert!(
        matches!(err, SidecarError::Deserialize(_)),
        "expected Deserialize, got: {err:?}"
    );
}

#[test]
fn codec_decode_wrong_tag_gives_deserialize_variant() {
    let err = JsonlCodec::decode(r#"{"t":"nonexistent","x":1}"#).unwrap_err();
    assert!(matches!(err, SidecarError::Deserialize(_)));
}

#[test]
fn codec_decode_missing_required_field_gives_deserialize_variant() {
    // "run" frame requires "id" and "work_order"
    let err = JsonlCodec::decode(r#"{"t":"run"}"#).unwrap_err();
    assert!(matches!(err, SidecarError::Deserialize(_)));
}

#[test]
fn codec_decode_error_message_contains_context() {
    let err = JsonlCodec::decode("not json").unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.starts_with("deserialization error:"),
        "unexpected message: {msg}"
    );
}

#[test]
fn codec_decode_error_source_is_serde_json() {
    let err = JsonlCodec::decode("not json").unwrap_err();
    let src = err.source().expect("should have source");
    // The source should be a serde_json::Error, downcastable
    assert!(src.downcast_ref::<serde_json::Error>().is_some());
}

// ── Frame typed extraction error paths ───────────────────────────────

#[test]
fn try_event_on_wrong_frame_gives_protocol() {
    let frame = Frame::Final {
        ref_id: "r1".into(),
        receipt: json!({}),
    };
    let err: SidecarError = frame.try_event::<Value>().unwrap_err();
    assert!(matches!(err, SidecarError::Protocol(_)));
    assert!(err.to_string().contains("expected Event frame"));
}

#[test]
fn try_final_on_wrong_frame_gives_protocol() {
    let frame = Frame::Event {
        ref_id: "r1".into(),
        event: json!({}),
    };
    let err: SidecarError = frame.try_final::<Value>().unwrap_err();
    assert!(matches!(err, SidecarError::Protocol(_)));
    assert!(err.to_string().contains("expected Final frame"));
}

#[test]
fn try_event_type_mismatch_gives_deserialize() {
    // Event payload is a string, but we try to deserialize as a struct
    let frame = Frame::Event {
        ref_id: "r1".into(),
        event: json!("just a string"),
    };
    #[derive(Debug, serde::Deserialize)]
    struct Specific {
        _field: i32,
    }
    let err: SidecarError = frame.try_event::<Specific>().unwrap_err();
    assert!(
        matches!(err, SidecarError::Deserialize(_)),
        "expected Deserialize, got: {err:?}"
    );
}

#[test]
fn try_final_type_mismatch_gives_deserialize() {
    let frame = Frame::Final {
        ref_id: "r1".into(),
        receipt: json!(42),
    };
    #[derive(Debug, serde::Deserialize)]
    struct Receipt {
        _status: String,
    }
    let err: SidecarError = frame.try_final::<Receipt>().unwrap_err();
    assert!(matches!(err, SidecarError::Deserialize(_)));
}

// ── Display coverage for all variants ────────────────────────────────

#[test]
fn all_variants_have_nonempty_display() {
    let io_err = || std::io::Error::new(std::io::ErrorKind::Other, "test");
    let serde_err = || serde_json::from_str::<()>("x").unwrap_err();

    let variants: Vec<SidecarError> = vec![
        SidecarError::Spawn(io_err()),
        SidecarError::Stdout(io_err()),
        SidecarError::Stdin(io_err()),
        SidecarError::Protocol("test".into()),
        SidecarError::Serialize(serde_err()),
        SidecarError::Deserialize(serde_err()),
        SidecarError::Fatal("test".into()),
        SidecarError::Exited(Some(1)),
        SidecarError::Exited(None),
        SidecarError::Timeout,
    ];

    for v in &variants {
        let msg = v.to_string();
        assert!(!msg.is_empty(), "empty display for {v:?}");
        assert!(msg.len() > 5, "suspiciously short display: {msg}");
    }
}

// ── Debug coverage ───────────────────────────────────────────────────

#[test]
fn all_variants_have_debug() {
    let io_err = || std::io::Error::new(std::io::ErrorKind::Other, "test");
    let serde_err = || serde_json::from_str::<()>("x").unwrap_err();

    let variants: Vec<SidecarError> = vec![
        SidecarError::Spawn(io_err()),
        SidecarError::Stdout(io_err()),
        SidecarError::Stdin(io_err()),
        SidecarError::Protocol("test".into()),
        SidecarError::Serialize(serde_err()),
        SidecarError::Deserialize(serde_err()),
        SidecarError::Fatal("test".into()),
        SidecarError::Exited(Some(1)),
        SidecarError::Exited(None),
        SidecarError::Timeout,
    ];

    for v in &variants {
        let dbg = format!("{v:?}");
        assert!(!dbg.is_empty());
    }
}

// ── Exhaustiveness guard ─────────────────────────────────────────────

/// Ensures the match covers every SidecarError variant.
/// If a new variant is added, this test will fail to compile.
#[test]
fn error_match_is_exhaustive() {
    let e = SidecarError::Timeout;
    match e {
        SidecarError::Spawn(_) => {}
        SidecarError::Stdout(_) => {}
        SidecarError::Stdin(_) => {}
        SidecarError::Protocol(_) => {}
        SidecarError::Serialize(_) => {}
        SidecarError::Deserialize(_) => {}
        SidecarError::Fatal(_) => {}
        SidecarError::Exited(_) => {}
        SidecarError::Timeout => {}
    }
}
