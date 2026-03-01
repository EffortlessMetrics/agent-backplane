// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property-based tests for `sidecar-kit` types.

use proptest::prelude::*;
use serde_json::Value;
use sidecar_kit::{Frame, JsonlCodec, ProcessSpec, SidecarError};

// ── Leaf strategies ─────────────────────────────────────────────────────

fn arb_string() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_ .-]{0,20}"
}

fn arb_nonempty_string() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_.-]{1,20}"
}

fn arb_json_value_simple() -> impl Strategy<Value = Value> {
    prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        arb_string().prop_map(Value::String),
        (-1000i64..1000).prop_map(|n| Value::Number(n.into())),
    ]
}

// ── Frame strategy ──────────────────────────────────────────────────────

fn arb_frame() -> impl Strategy<Value = Frame> {
    prop_oneof![
        (
            arb_nonempty_string(),
            arb_json_value_simple(),
            arb_json_value_simple(),
        )
            .prop_map(|(cv, backend, caps)| Frame::Hello {
                contract_version: cv,
                backend,
                capabilities: caps,
                mode: Value::Null,
            }),
        (arb_nonempty_string(), arb_json_value_simple())
            .prop_map(|(id, wo)| Frame::Run { id, work_order: wo }),
        (arb_nonempty_string(), arb_json_value_simple())
            .prop_map(|(ref_id, event)| Frame::Event { ref_id, event }),
        (arb_nonempty_string(), arb_json_value_simple())
            .prop_map(|(ref_id, receipt)| Frame::Final { ref_id, receipt }),
        (prop::option::of(arb_nonempty_string()), arb_string())
            .prop_map(|(ref_id, error)| Frame::Fatal { ref_id, error }),
        (arb_nonempty_string(), prop::option::of(arb_string()))
            .prop_map(|(ref_id, reason)| Frame::Cancel { ref_id, reason }),
        any::<u64>().prop_map(|seq| Frame::Ping { seq }),
        any::<u64>().prop_map(|seq| Frame::Pong { seq }),
    ]
}

// ── Property tests ──────────────────────────────────────────────────────

proptest! {
    /// `SidecarError::Protocol` display is never empty for any message string.
    #[test]
    fn error_display_never_empty_protocol(msg in arb_string()) {
        let err = SidecarError::Protocol(msg);
        prop_assert!(!err.to_string().is_empty());
    }

    /// `SidecarError::Fatal` display is never empty for any message string.
    #[test]
    fn error_display_never_empty_fatal(msg in arb_string()) {
        let err = SidecarError::Fatal(msg);
        prop_assert!(!err.to_string().is_empty());
    }

    /// `SidecarError::Exited` display is never empty for any exit code.
    #[test]
    fn error_display_never_empty_exited(code in prop::option::of(any::<i32>())) {
        let err = SidecarError::Exited(code);
        prop_assert!(!err.to_string().is_empty());
    }

    /// `SidecarError::Timeout` display is never empty.
    #[test]
    fn error_display_never_empty_timeout(_dummy in 0..1u8) {
        let err = SidecarError::Timeout;
        prop_assert!(!err.to_string().is_empty());
    }

    /// Any `Frame` serialized to JSON and back should produce an identical
    /// JSON value (round-trip).
    #[test]
    fn frame_roundtrip_json(frame in arb_frame()) {
        let json_str = serde_json::to_string(&frame).unwrap();
        let decoded: Frame = serde_json::from_str(&json_str).unwrap();
        let json_str2 = serde_json::to_string(&decoded).unwrap();

        let v1: Value = serde_json::from_str(&json_str).unwrap();
        let v2: Value = serde_json::from_str(&json_str2).unwrap();
        prop_assert_eq!(v1, v2);
    }

    /// `ProcessSpec::new` never panics for any arbitrary input string.
    #[test]
    fn process_spec_new_never_panics(cmd in ".*") {
        let spec = ProcessSpec::new(cmd.clone());
        prop_assert_eq!(spec.command, cmd);
        prop_assert!(spec.args.is_empty());
        prop_assert!(spec.env.is_empty());
        prop_assert!(spec.cwd.is_none());
    }

    /// Codec encode/decode round-trip: encoding a frame then decoding the
    /// resulting line should yield identical JSON.
    #[test]
    fn codec_roundtrip(frame in arb_frame()) {
        let encoded = JsonlCodec::encode(&frame).unwrap();
        prop_assert!(encoded.ends_with('\n'));
        prop_assert_eq!(encoded.matches('\n').count(), 1);

        let decoded = JsonlCodec::decode(encoded.trim_end()).unwrap();
        let v1 = serde_json::to_value(&frame).unwrap();
        let v2 = serde_json::to_value(&decoded).unwrap();
        prop_assert_eq!(v1, v2);
    }
}
