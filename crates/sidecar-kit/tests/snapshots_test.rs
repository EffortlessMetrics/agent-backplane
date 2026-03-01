// SPDX-License-Identifier: MIT OR Apache-2.0
//! Snapshot tests for `sidecar-kit` types.

use insta::{assert_json_snapshot, assert_snapshot};
use serde_json::{Value, json};
use sidecar_kit::{Frame, ProcessSpec, SidecarError};

// ── SidecarError Display snapshots ──────────────────────────────────────

#[test]
fn snapshot_error_protocol() {
    let err = SidecarError::Protocol("expected hello frame".into());
    assert_snapshot!("error_protocol", err.to_string());
}

#[test]
fn snapshot_error_fatal() {
    let err = SidecarError::Fatal("out of memory".into());
    assert_snapshot!("error_fatal", err.to_string());
}

#[test]
fn snapshot_error_exited_some() {
    let err = SidecarError::Exited(Some(1));
    assert_snapshot!("error_exited_some", err.to_string());
}

#[test]
fn snapshot_error_exited_none() {
    let err = SidecarError::Exited(None);
    assert_snapshot!("error_exited_none", err.to_string());
}

#[test]
fn snapshot_error_timeout() {
    let err = SidecarError::Timeout;
    assert_snapshot!("error_timeout", err.to_string());
}

// ── Frame serialization snapshots ───────────────────────────────────────

#[test]
fn snapshot_frame_hello() {
    let frame = Frame::Hello {
        contract_version: "abp/v0.1".into(),
        backend: json!({"id": "mock"}),
        capabilities: json!({"streaming": "native"}),
        mode: Value::Null,
    };
    let val: Value = serde_json::to_value(&frame).unwrap();
    assert_json_snapshot!("frame_hello", val);
}

#[test]
fn snapshot_frame_event() {
    let frame = Frame::Event {
        ref_id: "run-001".into(),
        event: json!({"type": "assistant_delta", "text": "Hello"}),
    };
    let val: Value = serde_json::to_value(&frame).unwrap();
    assert_json_snapshot!("frame_event", val);
}

#[test]
fn snapshot_frame_fatal() {
    let frame = Frame::Fatal {
        ref_id: Some("run-001".into()),
        error: "something went wrong".into(),
    };
    let val: Value = serde_json::to_value(&frame).unwrap();
    assert_json_snapshot!("frame_fatal", val);
}

// ── ProcessSpec snapshot ────────────────────────────────────────────────

#[test]
fn snapshot_process_spec() {
    let mut spec = ProcessSpec::new("node");
    spec.args = vec!["index.js".into(), "--verbose".into()];
    spec.env.insert("NODE_ENV".into(), "production".into());
    spec.cwd = Some("/tmp/work".into());
    assert_snapshot!("process_spec", format!("{:#?}", spec));
}
