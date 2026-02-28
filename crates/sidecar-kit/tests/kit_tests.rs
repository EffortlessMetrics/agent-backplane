use serde_json::{Value, json};
use sidecar_kit::{CancelToken, Frame, JsonlCodec, ProcessSpec, SidecarError};

// ── CancelToken ──────────────────────────────────────────────────────

#[test]
fn cancel_token_starts_uncancelled() {
    let token = CancelToken::new();
    assert!(!token.is_cancelled());
}

#[test]
fn cancel_token_cancel_sets_flag() {
    let token = CancelToken::new();
    token.cancel();
    assert!(token.is_cancelled());
}

#[test]
fn cancel_token_clone_shares_state() {
    let t1 = CancelToken::new();
    let t2 = t1.clone();
    assert!(!t2.is_cancelled());
    t1.cancel();
    assert!(t2.is_cancelled());
}

#[test]
fn cancel_token_default_is_uncancelled() {
    let token = CancelToken::default();
    assert!(!token.is_cancelled());
}

#[tokio::test]
async fn cancel_token_cancelled_future_returns_immediately_when_already_cancelled() {
    let token = CancelToken::new();
    token.cancel();
    // Should return immediately and not hang
    token.cancelled().await;
}

#[tokio::test]
async fn cancel_token_cancelled_future_resolves_on_cancel() {
    let token = CancelToken::new();
    let t2 = token.clone();
    let handle = tokio::spawn(async move {
        t2.cancelled().await;
        true
    });
    // Give the spawned task time to start waiting
    tokio::task::yield_now().await;
    token.cancel();
    assert!(handle.await.unwrap());
}

// ── Frame serialization ──────────────────────────────────────────────

#[test]
fn frame_hello_round_trip() {
    let frame = Frame::Hello {
        contract_version: "abp/v0.1".into(),
        backend: json!({"name": "test"}),
        capabilities: json!({"tools": true}),
        mode: json!("mapped"),
    };
    let encoded = JsonlCodec::encode(&frame).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();

    match decoded {
        Frame::Hello {
            contract_version,
            backend,
            capabilities,
            mode,
        } => {
            assert_eq!(contract_version, "abp/v0.1");
            assert_eq!(backend, json!({"name": "test"}));
            assert_eq!(capabilities, json!({"tools": true}));
            assert_eq!(mode, json!("mapped"));
        }
        _ => panic!("expected Hello frame"),
    }
}

#[test]
fn frame_run_round_trip() {
    let frame = Frame::Run {
        id: "run-42".into(),
        work_order: json!({"task": "build"}),
    };
    let encoded = JsonlCodec::encode(&frame).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();

    match decoded {
        Frame::Run { id, work_order } => {
            assert_eq!(id, "run-42");
            assert_eq!(work_order, json!({"task": "build"}));
        }
        _ => panic!("expected Run frame"),
    }
}

#[test]
fn frame_event_round_trip() {
    let frame = Frame::Event {
        ref_id: "run-42".into(),
        event: json!({"type": "progress", "pct": 50}),
    };
    let encoded = JsonlCodec::encode(&frame).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();

    match decoded {
        Frame::Event { ref_id, event } => {
            assert_eq!(ref_id, "run-42");
            assert_eq!(event["pct"], 50);
        }
        _ => panic!("expected Event frame"),
    }
}

#[test]
fn frame_final_round_trip() {
    let frame = Frame::Final {
        ref_id: "run-42".into(),
        receipt: json!({"status": "ok"}),
    };
    let encoded = JsonlCodec::encode(&frame).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();

    match decoded {
        Frame::Final { ref_id, receipt } => {
            assert_eq!(ref_id, "run-42");
            assert_eq!(receipt, json!({"status": "ok"}));
        }
        _ => panic!("expected Final frame"),
    }
}

#[test]
fn frame_fatal_round_trip() {
    let frame = Frame::Fatal {
        ref_id: Some("run-42".into()),
        error: "something broke".into(),
    };
    let encoded = JsonlCodec::encode(&frame).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();

    match decoded {
        Frame::Fatal { ref_id, error } => {
            assert_eq!(ref_id, Some("run-42".into()));
            assert_eq!(error, "something broke");
        }
        _ => panic!("expected Fatal frame"),
    }
}

#[test]
fn frame_fatal_no_ref_id_round_trip() {
    let frame = Frame::Fatal {
        ref_id: None,
        error: "global error".into(),
    };
    let encoded = JsonlCodec::encode(&frame).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();

    match decoded {
        Frame::Fatal { ref_id, error } => {
            assert!(ref_id.is_none());
            assert_eq!(error, "global error");
        }
        _ => panic!("expected Fatal frame"),
    }
}

#[test]
fn frame_cancel_round_trip() {
    let frame = Frame::Cancel {
        ref_id: "run-42".into(),
        reason: Some("user requested".into()),
    };
    let encoded = JsonlCodec::encode(&frame).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();

    match decoded {
        Frame::Cancel { ref_id, reason } => {
            assert_eq!(ref_id, "run-42");
            assert_eq!(reason, Some("user requested".into()));
        }
        _ => panic!("expected Cancel frame"),
    }
}

#[test]
fn frame_ping_pong_round_trip() {
    let ping = Frame::Ping { seq: 7 };
    let encoded = JsonlCodec::encode(&ping).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Frame::Ping { seq } => assert_eq!(seq, 7),
        _ => panic!("expected Ping frame"),
    }

    let pong = Frame::Pong { seq: 7 };
    let encoded = JsonlCodec::encode(&pong).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Frame::Pong { seq } => assert_eq!(seq, 7),
        _ => panic!("expected Pong frame"),
    }
}

// ── Frame tag discriminator ("t") ────────────────────────────────────

#[test]
fn frame_hello_has_t_tag() {
    let frame = Frame::Hello {
        contract_version: "abp/v0.1".into(),
        backend: json!(null),
        capabilities: json!(null),
        mode: Value::Null,
    };
    let json_str = serde_json::to_string(&frame).unwrap();
    let v: Value = serde_json::from_str(&json_str).unwrap();
    assert_eq!(v["t"], "hello");
}

#[test]
fn frame_run_has_t_tag() {
    let frame = Frame::Run {
        id: "r1".into(),
        work_order: json!(null),
    };
    let v: Value = serde_json::to_value(&frame).unwrap();
    assert_eq!(v["t"], "run");
}

#[test]
fn frame_event_has_t_tag() {
    let frame = Frame::Event {
        ref_id: "r1".into(),
        event: json!(null),
    };
    let v: Value = serde_json::to_value(&frame).unwrap();
    assert_eq!(v["t"], "event");
}

#[test]
fn frame_final_has_t_tag() {
    let frame = Frame::Final {
        ref_id: "r1".into(),
        receipt: json!(null),
    };
    let v: Value = serde_json::to_value(&frame).unwrap();
    assert_eq!(v["t"], "final");
}

#[test]
fn frame_fatal_has_t_tag() {
    let frame = Frame::Fatal {
        ref_id: None,
        error: "err".into(),
    };
    let v: Value = serde_json::to_value(&frame).unwrap();
    assert_eq!(v["t"], "fatal");
}

#[test]
fn frame_cancel_has_t_tag() {
    let frame = Frame::Cancel {
        ref_id: "r1".into(),
        reason: None,
    };
    let v: Value = serde_json::to_value(&frame).unwrap();
    assert_eq!(v["t"], "cancel");
}

#[test]
fn frame_ping_has_t_tag() {
    let v: Value = serde_json::to_value(&Frame::Ping { seq: 0 }).unwrap();
    assert_eq!(v["t"], "ping");
}

#[test]
fn frame_pong_has_t_tag() {
    let v: Value = serde_json::to_value(&Frame::Pong { seq: 0 }).unwrap();
    assert_eq!(v["t"], "pong");
}

// ── Frame typed extraction helpers ───────────────────────────────────

#[test]
fn frame_try_event_extracts_typed_value() {
    let frame = Frame::Event {
        ref_id: "r1".into(),
        event: json!({"count": 42}),
    };
    let (ref_id, val): (String, Value) = frame.try_event().unwrap();
    assert_eq!(ref_id, "r1");
    assert_eq!(val["count"], 42);
}

#[test]
fn frame_try_event_on_non_event_returns_error() {
    let frame = Frame::Ping { seq: 1 };
    let result: Result<(String, Value), _> = frame.try_event();
    assert!(result.is_err());
}

#[test]
fn frame_try_final_extracts_typed_value() {
    let frame = Frame::Final {
        ref_id: "r1".into(),
        receipt: json!({"done": true}),
    };
    let (ref_id, val): (String, Value) = frame.try_final().unwrap();
    assert_eq!(ref_id, "r1");
    assert_eq!(val["done"], true);
}

#[test]
fn frame_try_final_on_non_final_returns_error() {
    let frame = Frame::Ping { seq: 1 };
    let result: Result<(String, Value), _> = frame.try_final();
    assert!(result.is_err());
}

// ── JsonlCodec ───────────────────────────────────────────────────────

#[test]
fn codec_encode_appends_newline() {
    let frame = Frame::Ping { seq: 1 };
    let encoded = JsonlCodec::encode(&frame).unwrap();
    assert!(encoded.ends_with('\n'));
}

#[test]
fn codec_decode_invalid_json_returns_error() {
    let result = JsonlCodec::decode("not json at all");
    assert!(result.is_err());
}

#[test]
fn codec_decode_empty_string_returns_error() {
    let result = JsonlCodec::decode("");
    assert!(result.is_err());
}

#[test]
fn codec_decode_whitespace_only_returns_error() {
    let result = JsonlCodec::decode("   ");
    assert!(result.is_err());
}

#[test]
fn codec_decode_valid_json_missing_tag_returns_error() {
    let result = JsonlCodec::decode(r#"{"id":"1"}"#);
    assert!(result.is_err());
}

#[test]
fn codec_decode_unknown_tag_returns_error() {
    let result = JsonlCodec::decode(r#"{"t":"unknown_variant","data":1}"#);
    assert!(result.is_err());
}

#[test]
fn codec_round_trip_preserves_nested_values() {
    let frame = Frame::Event {
        ref_id: "r1".into(),
        event: json!({
            "deeply": {"nested": {"value": [1, 2, 3]}},
            "unicode": "héllo 🌍"
        }),
    };
    let encoded = JsonlCodec::encode(&frame).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    match decoded {
        Frame::Event { event, .. } => {
            assert_eq!(event["deeply"]["nested"]["value"], json!([1, 2, 3]));
            assert_eq!(event["unicode"], "héllo 🌍");
        }
        _ => panic!("expected Event"),
    }
}

// ── ProcessSpec ──────────────────────────────────────────────────────

#[test]
fn process_spec_new_sets_command() {
    let spec = ProcessSpec::new("node");
    assert_eq!(spec.command, "node");
    assert!(spec.args.is_empty());
    assert!(spec.env.is_empty());
    assert!(spec.cwd.is_none());
}

#[test]
fn process_spec_fields_are_mutable() {
    let mut spec = ProcessSpec::new("python");
    spec.args.push("script.py".into());
    spec.env.insert("FOO".into(), "bar".into());
    spec.cwd = Some("/tmp".into());

    assert_eq!(spec.command, "python");
    assert_eq!(spec.args, vec!["script.py"]);
    assert_eq!(spec.env.get("FOO").unwrap(), "bar");
    assert_eq!(spec.cwd, Some("/tmp".into()));
}

#[test]
fn process_spec_clone() {
    let mut spec = ProcessSpec::new("node");
    spec.args.push("index.js".into());
    let cloned = spec.clone();
    assert_eq!(cloned.command, "node");
    assert_eq!(cloned.args, vec!["index.js"]);
}

#[test]
fn process_spec_env_is_btreemap() {
    let mut spec = ProcessSpec::new("sh");
    spec.env.insert("B".into(), "2".into());
    spec.env.insert("A".into(), "1".into());
    // BTreeMap is sorted by key
    let keys: Vec<_> = spec.env.keys().collect();
    assert_eq!(keys, vec!["A", "B"]);
}

// ── SidecarError Display ─────────────────────────────────────────────

#[test]
fn error_protocol_display() {
    let e = SidecarError::Protocol("bad handshake".into());
    assert_eq!(e.to_string(), "protocol violation: bad handshake");
}

#[test]
fn error_serialize_display() {
    let e = SidecarError::Serialize("failed".into());
    assert_eq!(e.to_string(), "serialization error: failed");
}

#[test]
fn error_deserialize_display() {
    let e = SidecarError::Deserialize("invalid".into());
    assert_eq!(e.to_string(), "deserialization error: invalid");
}

#[test]
fn error_fatal_display() {
    let e = SidecarError::Fatal("crash".into());
    assert_eq!(e.to_string(), "sidecar fatal error: crash");
}

#[test]
fn error_timeout_display() {
    let e = SidecarError::Timeout;
    assert_eq!(e.to_string(), "operation timed out");
}

#[test]
fn error_exited_with_code_display() {
    let e = SidecarError::Exited(Some(1));
    assert_eq!(e.to_string(), "sidecar exited unexpectedly (code=Some(1))");
}

#[test]
fn error_exited_no_code_display() {
    let e = SidecarError::Exited(None);
    assert_eq!(e.to_string(), "sidecar exited unexpectedly (code=None)");
}

#[test]
fn error_spawn_display() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "not found");
    let e = SidecarError::Spawn(io_err);
    assert!(e.to_string().starts_with("failed to spawn sidecar:"));
}

#[test]
fn error_stdout_display() {
    let io_err = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broke");
    let e = SidecarError::Stdout(io_err);
    assert!(e.to_string().starts_with("failed to read sidecar stdout:"));
}

#[test]
fn error_stdin_display() {
    let io_err = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broke");
    let e = SidecarError::Stdin(io_err);
    assert!(e.to_string().starts_with("failed to write sidecar stdin:"));
}

#[test]
fn error_is_debug() {
    let e = SidecarError::Timeout;
    let dbg = format!("{:?}", e);
    assert!(dbg.contains("Timeout"));
}

// ── Frame decode from raw JSON strings ───────────────────────────────

#[test]
fn decode_hello_from_raw_json() {
    let raw = r#"{"t":"hello","contract_version":"abp/v0.1","backend":"mock","capabilities":{}}"#;
    let frame = JsonlCodec::decode(raw).unwrap();
    match frame {
        Frame::Hello {
            contract_version,
            backend,
            ..
        } => {
            assert_eq!(contract_version, "abp/v0.1");
            assert_eq!(backend, json!("mock"));
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn decode_hello_mode_defaults_to_null() {
    let raw = r#"{"t":"hello","contract_version":"v1","backend":null,"capabilities":null}"#;
    let frame = JsonlCodec::decode(raw).unwrap();
    match frame {
        Frame::Hello { mode, .. } => assert_eq!(mode, Value::Null),
        _ => panic!("expected Hello"),
    }
}
