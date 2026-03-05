#![allow(clippy::all)]
#![allow(clippy::manual_repeat_n)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::single_component_path_imports)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unnecessary_to_owned)]
#![allow(clippy::implicit_clone)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::iter_kv_map)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::single_match)]
#![allow(clippy::manual_map)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::len_zero)]
#![allow(clippy::map_entry)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! Deep tests for sidecar-kit protocol utilities: framing, validation,
//! state machine, and edge cases.

use std::io::BufReader;

use serde_json::{Value, json};
use sidecar_kit::{
    Frame, FrameReader, FrameWriter, JsonlCodec, ProtocolPhase, ProtocolState,
    builders::{event_frame, event_text_delta, fatal_frame, hello_frame},
    framing::{
        DEFAULT_MAX_FRAME_SIZE, buf_reader_from_bytes, frame_to_json, json_to_frame,
        read_all_frames, validate_frame, write_frames,
    },
};

// ═══════════════════════════════════════════════════════════════════════
// Module: frame_writer (~10 tests)
// ═══════════════════════════════════════════════════════════════════════
mod frame_writer {
    use super::*;

    #[test]
    fn write_hello_frame_produces_valid_jsonl() {
        let mut buf = Vec::new();
        let mut w = FrameWriter::new(&mut buf);
        let frame = hello_frame("test-backend");
        w.write_frame(&frame).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.ends_with('\n'));
        assert!(output.contains(r#""t":"hello""#));
    }

    #[test]
    fn write_multiple_frames_produces_separate_lines() {
        let mut buf = Vec::new();
        let mut w = FrameWriter::new(&mut buf);
        w.write_frame(&hello_frame("b1")).unwrap();
        w.write_frame(&fatal_frame(None, "oops")).unwrap();
        let output = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn frames_written_counter_increments() {
        let mut buf = Vec::new();
        let mut w = FrameWriter::new(&mut buf);
        assert_eq!(w.frames_written(), 0);
        w.write_frame(&hello_frame("x")).unwrap();
        assert_eq!(w.frames_written(), 1);
        w.write_frame(&hello_frame("y")).unwrap();
        assert_eq!(w.frames_written(), 2);
    }

    #[test]
    fn oversized_frame_rejected() {
        let mut buf = Vec::new();
        let mut w = FrameWriter::with_max_size(&mut buf, 32);
        let frame = hello_frame("test-backend");
        let err = w.write_frame(&frame).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("exceeds limit"));
    }

    #[test]
    fn flush_does_not_error_on_vec() {
        let mut buf = Vec::new();
        let mut w = FrameWriter::new(&mut buf);
        w.write_frame(&hello_frame("x")).unwrap();
        w.flush().unwrap();
    }

    #[test]
    fn into_inner_returns_writer() {
        let buf = Vec::new();
        let w = FrameWriter::new(buf);
        let recovered = w.into_inner();
        assert!(recovered.is_empty());
    }

    #[test]
    fn inner_borrows_writer() {
        let buf = Vec::new();
        let w = FrameWriter::new(buf);
        let _ = w.inner();
    }

    #[test]
    fn write_event_frame_includes_ref_id() {
        let mut buf = Vec::new();
        let mut w = FrameWriter::new(&mut buf);
        let frame = event_frame("run-42", event_text_delta("hi"));
        w.write_frame(&frame).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("run-42"));
    }

    #[test]
    fn write_frame_serializes_tag_as_t() {
        let mut buf = Vec::new();
        let mut w = FrameWriter::new(&mut buf);
        w.write_frame(&Frame::Ping { seq: 1 }).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains(r#""t":"ping""#));
        assert!(!output.contains(r#""type":"ping""#));
    }

    #[test]
    fn write_frames_helper_writes_and_counts() {
        let mut buf = Vec::new();
        let frames = vec![hello_frame("a"), fatal_frame(None, "err")];
        let count = write_frames(&mut buf, &frames).unwrap();
        assert_eq!(count, 2);
        let output = String::from_utf8(buf).unwrap();
        assert_eq!(output.lines().count(), 2);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: frame_reader (~12 tests)
// ═══════════════════════════════════════════════════════════════════════
mod frame_reader {
    use super::*;

    fn make_input(frames: &[Frame]) -> Vec<u8> {
        let mut buf = Vec::new();
        for f in frames {
            let mut line = serde_json::to_string(f).unwrap();
            line.push('\n');
            buf.extend_from_slice(line.as_bytes());
        }
        buf
    }

    #[test]
    fn read_single_frame() {
        let data = make_input(&[hello_frame("test")]);
        let mut r = FrameReader::new(BufReader::new(data.as_slice()));
        let frame = r.read_frame().unwrap().unwrap();
        assert!(matches!(frame, Frame::Hello { .. }));
    }

    #[test]
    fn read_returns_none_on_eof() {
        let data: &[u8] = b"";
        let mut r = FrameReader::new(BufReader::new(data));
        assert!(r.read_frame().unwrap().is_none());
    }

    #[test]
    fn read_skips_blank_lines() {
        let mut data = b"\n\n".to_vec();
        let hello = serde_json::to_string(&hello_frame("b")).unwrap();
        data.extend_from_slice(hello.as_bytes());
        data.push(b'\n');
        data.extend_from_slice(b"\n");
        let mut r = FrameReader::new(BufReader::new(data.as_slice()));
        let frame = r.read_frame().unwrap().unwrap();
        assert!(matches!(frame, Frame::Hello { .. }));
        assert!(r.read_frame().unwrap().is_none());
    }

    #[test]
    fn read_multiple_frames() {
        let frames = vec![
            hello_frame("b"),
            Frame::Run {
                id: "r1".into(),
                work_order: json!({"task": "test"}),
            },
            fatal_frame(Some("r1"), "done"),
        ];
        let data = make_input(&frames);
        let mut r = FrameReader::new(BufReader::new(data.as_slice()));
        assert!(matches!(
            r.read_frame().unwrap().unwrap(),
            Frame::Hello { .. }
        ));
        assert!(matches!(
            r.read_frame().unwrap().unwrap(),
            Frame::Run { .. }
        ));
        assert!(matches!(
            r.read_frame().unwrap().unwrap(),
            Frame::Fatal { .. }
        ));
        assert!(r.read_frame().unwrap().is_none());
    }

    #[test]
    fn frames_read_counter() {
        let data = make_input(&[hello_frame("b"), hello_frame("b2")]);
        let mut r = FrameReader::new(BufReader::new(data.as_slice()));
        assert_eq!(r.frames_read(), 0);
        r.read_frame().unwrap();
        assert_eq!(r.frames_read(), 1);
        r.read_frame().unwrap();
        assert_eq!(r.frames_read(), 2);
    }

    #[test]
    fn oversized_frame_rejected() {
        let data = make_input(&[hello_frame("backend")]);
        let mut r = FrameReader::with_max_size(BufReader::new(data.as_slice()), 10);
        let err = r.read_frame().unwrap_err();
        assert!(err.to_string().contains("exceeds limit"));
    }

    #[test]
    fn invalid_json_rejected() {
        let data = b"this is not json\n";
        let mut r = FrameReader::new(BufReader::new(data.as_slice()));
        let err = r.read_frame().unwrap_err();
        assert!(err.to_string().contains("error"));
    }

    #[test]
    fn frames_iterator_collects_all() {
        let data = make_input(&[hello_frame("a"), hello_frame("b")]);
        let r = FrameReader::new(BufReader::new(data.as_slice()));
        let all: Vec<_> = r.frames().collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn frames_iterator_empty_input() {
        let data: &[u8] = b"";
        let r = FrameReader::new(BufReader::new(data));
        let all: Vec<_> = r.frames().collect::<Result<Vec<_>, _>>().unwrap();
        assert!(all.is_empty());
    }

    #[test]
    fn read_all_frames_helper() {
        let data = make_input(&[hello_frame("x")]);
        let all = read_all_frames(BufReader::new(data.as_slice())).unwrap();
        assert_eq!(all.len(), 1);
    }

    #[test]
    fn read_all_frames_empty() {
        let data: &[u8] = b"";
        let all = read_all_frames(BufReader::new(data)).unwrap();
        assert!(all.is_empty());
    }

    #[test]
    fn buf_reader_from_bytes_helper() {
        let data = b"hello\n";
        let _r = buf_reader_from_bytes(data);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: frame_roundtrip (~8 tests)
// ═══════════════════════════════════════════════════════════════════════
mod frame_roundtrip {
    use super::*;

    fn roundtrip(frame: &Frame) -> Frame {
        let mut buf = Vec::new();
        let mut w = FrameWriter::new(&mut buf);
        w.write_frame(frame).unwrap();
        let mut r = FrameReader::new(BufReader::new(buf.as_slice()));
        r.read_frame().unwrap().unwrap()
    }

    #[test]
    fn hello_roundtrip() {
        let frame = hello_frame("backend-1");
        let out = roundtrip(&frame);
        match out {
            Frame::Hello {
                contract_version,
                backend,
                ..
            } => {
                assert_eq!(contract_version, "abp/v0.1");
                assert_eq!(backend["id"], "backend-1");
            }
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn run_roundtrip() {
        let frame = Frame::Run {
            id: "run-99".into(),
            work_order: json!({"task": "summarize"}),
        };
        let out = roundtrip(&frame);
        match out {
            Frame::Run { id, work_order } => {
                assert_eq!(id, "run-99");
                assert_eq!(work_order["task"], "summarize");
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn event_roundtrip() {
        let frame = event_frame("r1", event_text_delta("hello"));
        let out = roundtrip(&frame);
        match out {
            Frame::Event { ref_id, event } => {
                assert_eq!(ref_id, "r1");
                assert_eq!(event["type"], "assistant_delta");
            }
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn final_roundtrip() {
        let frame = Frame::Final {
            ref_id: "r1".into(),
            receipt: json!({"outcome": "complete"}),
        };
        let out = roundtrip(&frame);
        match out {
            Frame::Final { ref_id, receipt } => {
                assert_eq!(ref_id, "r1");
                assert_eq!(receipt["outcome"], "complete");
            }
            _ => panic!("expected Final"),
        }
    }

    #[test]
    fn fatal_with_ref_id_roundtrip() {
        let frame = fatal_frame(Some("r1"), "oom");
        let out = roundtrip(&frame);
        match out {
            Frame::Fatal { ref_id, error } => {
                assert_eq!(ref_id, Some("r1".to_string()));
                assert_eq!(error, "oom");
            }
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn fatal_no_ref_id_roundtrip() {
        let frame = fatal_frame(None, "crash");
        let out = roundtrip(&frame);
        match out {
            Frame::Fatal { ref_id, error } => {
                assert!(ref_id.is_none());
                assert_eq!(error, "crash");
            }
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn ping_roundtrip() {
        let frame = Frame::Ping { seq: 42 };
        let out = roundtrip(&frame);
        match out {
            Frame::Ping { seq } => assert_eq!(seq, 42),
            _ => panic!("expected Ping"),
        }
    }

    #[test]
    fn pong_roundtrip() {
        let frame = Frame::Pong { seq: 7 };
        let out = roundtrip(&frame);
        match out {
            Frame::Pong { seq } => assert_eq!(seq, 7),
            _ => panic!("expected Pong"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: validate_frame (~12 tests)
// ═══════════════════════════════════════════════════════════════════════
mod validate_frame_tests {
    use super::*;

    #[test]
    fn valid_hello_passes() {
        let frame = hello_frame("my-backend");
        let result = validate_frame(&frame, DEFAULT_MAX_FRAME_SIZE);
        assert!(result.valid);
        assert!(result.issues.is_empty());
    }

    #[test]
    fn hello_empty_contract_version_fails() {
        let frame = Frame::Hello {
            contract_version: String::new(),
            backend: json!({"id": "x"}),
            capabilities: json!({}),
            mode: Value::Null,
        };
        let result = validate_frame(&frame, DEFAULT_MAX_FRAME_SIZE);
        assert!(!result.valid);
        assert!(result.issues.iter().any(|i| i.contains("contract_version")));
    }

    #[test]
    fn hello_bad_contract_version_prefix_fails() {
        let frame = Frame::Hello {
            contract_version: "v1.0".into(),
            backend: json!({"id": "x"}),
            capabilities: json!({}),
            mode: Value::Null,
        };
        let result = validate_frame(&frame, DEFAULT_MAX_FRAME_SIZE);
        assert!(!result.valid);
        assert!(result.issues.iter().any(|i| i.contains("abp/v")));
    }

    #[test]
    fn hello_missing_backend_id_fails() {
        let frame = Frame::Hello {
            contract_version: "abp/v0.1".into(),
            backend: json!({}),
            capabilities: json!({}),
            mode: Value::Null,
        };
        let result = validate_frame(&frame, DEFAULT_MAX_FRAME_SIZE);
        assert!(!result.valid);
        assert!(result.issues.iter().any(|i| i.contains("backend.id")));
    }

    #[test]
    fn run_empty_id_fails() {
        let frame = Frame::Run {
            id: String::new(),
            work_order: json!({"task": "test"}),
        };
        let result = validate_frame(&frame, DEFAULT_MAX_FRAME_SIZE);
        assert!(!result.valid);
        assert!(result.issues.iter().any(|i| i.contains("run id")));
    }

    #[test]
    fn valid_run_passes() {
        let frame = Frame::Run {
            id: "r1".into(),
            work_order: json!({}),
        };
        let result = validate_frame(&frame, DEFAULT_MAX_FRAME_SIZE);
        assert!(result.valid);
    }

    #[test]
    fn event_empty_ref_id_fails() {
        let frame = Frame::Event {
            ref_id: String::new(),
            event: json!({}),
        };
        let result = validate_frame(&frame, DEFAULT_MAX_FRAME_SIZE);
        assert!(!result.valid);
        assert!(result.issues.iter().any(|i| i.contains("ref_id")));
    }

    #[test]
    fn final_empty_ref_id_fails() {
        let frame = Frame::Final {
            ref_id: String::new(),
            receipt: json!({}),
        };
        let result = validate_frame(&frame, DEFAULT_MAX_FRAME_SIZE);
        assert!(!result.valid);
    }

    #[test]
    fn fatal_empty_error_fails() {
        let frame = Frame::Fatal {
            ref_id: None,
            error: String::new(),
        };
        let result = validate_frame(&frame, DEFAULT_MAX_FRAME_SIZE);
        assert!(!result.valid);
        assert!(result.issues.iter().any(|i| i.contains("error message")));
    }

    #[test]
    fn cancel_empty_ref_id_fails() {
        let frame = Frame::Cancel {
            ref_id: String::new(),
            reason: None,
        };
        let result = validate_frame(&frame, DEFAULT_MAX_FRAME_SIZE);
        assert!(!result.valid);
    }

    #[test]
    fn ping_always_valid() {
        let frame = Frame::Ping { seq: 0 };
        let result = validate_frame(&frame, DEFAULT_MAX_FRAME_SIZE);
        assert!(result.valid);
    }

    #[test]
    fn oversized_frame_flagged() {
        let frame = hello_frame("b");
        let result = validate_frame(&frame, 10);
        assert!(!result.valid);
        assert!(result.issues.iter().any(|i| i.contains("exceeds limit")));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: protocol_state_machine (~18 tests)
// ═══════════════════════════════════════════════════════════════════════
mod protocol_state_machine {
    use super::*;

    #[test]
    fn initial_state_is_awaiting_hello() {
        let s = ProtocolState::new();
        assert_eq!(s.phase(), ProtocolPhase::AwaitingHello);
        assert!(s.run_id().is_none());
        assert_eq!(s.events_seen(), 0);
        assert!(!s.is_terminal());
    }

    #[test]
    fn default_trait_matches_new() {
        let s = ProtocolState::default();
        assert_eq!(s.phase(), ProtocolPhase::AwaitingHello);
    }

    #[test]
    fn hello_transitions_to_awaiting_run() {
        let mut s = ProtocolState::new();
        s.advance(&hello_frame("b")).unwrap();
        assert_eq!(s.phase(), ProtocolPhase::AwaitingRun);
    }

    #[test]
    fn run_transitions_to_streaming() {
        let mut s = ProtocolState::new();
        s.advance(&hello_frame("b")).unwrap();
        s.advance(&Frame::Run {
            id: "r1".into(),
            work_order: json!({}),
        })
        .unwrap();
        assert_eq!(s.phase(), ProtocolPhase::Streaming);
        assert_eq!(s.run_id(), Some("r1"));
    }

    #[test]
    fn events_increment_counter() {
        let mut s = ProtocolState::new();
        s.advance(&hello_frame("b")).unwrap();
        s.advance(&Frame::Run {
            id: "r1".into(),
            work_order: json!({}),
        })
        .unwrap();
        s.advance(&event_frame("r1", json!({}))).unwrap();
        s.advance(&event_frame("r1", json!({}))).unwrap();
        assert_eq!(s.events_seen(), 2);
    }

    #[test]
    fn final_completes_protocol() {
        let mut s = ProtocolState::new();
        s.advance(&hello_frame("b")).unwrap();
        s.advance(&Frame::Run {
            id: "r1".into(),
            work_order: json!({}),
        })
        .unwrap();
        s.advance(&Frame::Final {
            ref_id: "r1".into(),
            receipt: json!({}),
        })
        .unwrap();
        assert_eq!(s.phase(), ProtocolPhase::Completed);
        assert!(s.is_terminal());
    }

    #[test]
    fn fatal_during_streaming_completes() {
        let mut s = ProtocolState::new();
        s.advance(&hello_frame("b")).unwrap();
        s.advance(&Frame::Run {
            id: "r1".into(),
            work_order: json!({}),
        })
        .unwrap();
        s.advance(&fatal_frame(Some("r1"), "error")).unwrap();
        assert_eq!(s.phase(), ProtocolPhase::Completed);
    }

    #[test]
    fn fatal_before_run_completes() {
        let mut s = ProtocolState::new();
        s.advance(&hello_frame("b")).unwrap();
        s.advance(&fatal_frame(None, "startup error")).unwrap();
        assert_eq!(s.phase(), ProtocolPhase::Completed);
    }

    #[test]
    fn event_before_hello_faults() {
        let mut s = ProtocolState::new();
        let err = s.advance(&event_frame("r1", json!({}))).unwrap_err();
        assert!(err.to_string().contains("expected hello"));
        assert_eq!(s.phase(), ProtocolPhase::Faulted);
    }

    #[test]
    fn run_before_hello_faults() {
        let mut s = ProtocolState::new();
        let err = s
            .advance(&Frame::Run {
                id: "r1".into(),
                work_order: json!({}),
            })
            .unwrap_err();
        assert!(err.to_string().contains("expected hello"));
        assert_eq!(s.phase(), ProtocolPhase::Faulted);
    }

    #[test]
    fn event_before_run_faults() {
        let mut s = ProtocolState::new();
        s.advance(&hello_frame("b")).unwrap();
        let err = s.advance(&event_frame("r1", json!({}))).unwrap_err();
        assert!(err.to_string().contains("expected run"));
        assert_eq!(s.phase(), ProtocolPhase::Faulted);
    }

    #[test]
    fn hello_after_hello_faults() {
        let mut s = ProtocolState::new();
        s.advance(&hello_frame("b")).unwrap();
        let err = s.advance(&hello_frame("b2")).unwrap_err();
        assert!(err.to_string().contains("expected run"));
        assert_eq!(s.phase(), ProtocolPhase::Faulted);
    }

    #[test]
    fn frame_after_completed_faults() {
        let mut s = ProtocolState::new();
        s.advance(&hello_frame("b")).unwrap();
        s.advance(&Frame::Run {
            id: "r1".into(),
            work_order: json!({}),
        })
        .unwrap();
        s.advance(&Frame::Final {
            ref_id: "r1".into(),
            receipt: json!({}),
        })
        .unwrap();
        let err = s.advance(&event_frame("r1", json!({}))).unwrap_err();
        assert!(err.to_string().contains("already completed"));
    }

    #[test]
    fn faulted_state_rejects_further_frames() {
        let mut s = ProtocolState::new();
        let _ = s.advance(&event_frame("r1", json!({})));
        assert_eq!(s.phase(), ProtocolPhase::Faulted);
        let err = s.advance(&hello_frame("b")).unwrap_err();
        assert!(err.to_string().contains("faulted"));
    }

    #[test]
    fn fault_reason_is_set() {
        let mut s = ProtocolState::new();
        let _ = s.advance(&event_frame("r1", json!({})));
        assert!(s.fault_reason().is_some());
        assert!(s.fault_reason().unwrap().contains("expected hello"));
    }

    #[test]
    fn reset_clears_state() {
        let mut s = ProtocolState::new();
        s.advance(&hello_frame("b")).unwrap();
        s.advance(&Frame::Run {
            id: "r1".into(),
            work_order: json!({}),
        })
        .unwrap();
        s.advance(&event_frame("r1", json!({}))).unwrap();
        s.reset();
        assert_eq!(s.phase(), ProtocolPhase::AwaitingHello);
        assert!(s.run_id().is_none());
        assert_eq!(s.events_seen(), 0);
        assert!(s.fault_reason().is_none());
    }

    #[test]
    fn reset_from_faulted_works() {
        let mut s = ProtocolState::new();
        let _ = s.advance(&event_frame("r1", json!({})));
        assert_eq!(s.phase(), ProtocolPhase::Faulted);
        s.reset();
        assert_eq!(s.phase(), ProtocolPhase::AwaitingHello);
        s.advance(&hello_frame("b")).unwrap();
        assert_eq!(s.phase(), ProtocolPhase::AwaitingRun);
    }

    #[test]
    fn ping_allowed_during_streaming() {
        let mut s = ProtocolState::new();
        s.advance(&hello_frame("b")).unwrap();
        s.advance(&Frame::Run {
            id: "r1".into(),
            work_order: json!({}),
        })
        .unwrap();
        s.advance(&Frame::Ping { seq: 1 }).unwrap();
        s.advance(&Frame::Pong { seq: 1 }).unwrap();
        assert_eq!(s.phase(), ProtocolPhase::Streaming);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: ref_id_correlation (~6 tests)
// ═══════════════════════════════════════════════════════════════════════
mod ref_id_correlation {
    use super::*;

    #[test]
    fn matching_ref_id_accepted() {
        let mut s = ProtocolState::new();
        s.advance(&hello_frame("b")).unwrap();
        s.advance(&Frame::Run {
            id: "r1".into(),
            work_order: json!({}),
        })
        .unwrap();
        s.advance(&event_frame("r1", json!({}))).unwrap();
        assert_eq!(s.events_seen(), 1);
    }

    #[test]
    fn mismatched_event_ref_id_rejected() {
        let mut s = ProtocolState::new();
        s.advance(&hello_frame("b")).unwrap();
        s.advance(&Frame::Run {
            id: "r1".into(),
            work_order: json!({}),
        })
        .unwrap();
        let err = s.advance(&event_frame("r-wrong", json!({}))).unwrap_err();
        assert!(err.to_string().contains("ref_id mismatch"));
    }

    #[test]
    fn mismatched_final_ref_id_rejected() {
        let mut s = ProtocolState::new();
        s.advance(&hello_frame("b")).unwrap();
        s.advance(&Frame::Run {
            id: "r1".into(),
            work_order: json!({}),
        })
        .unwrap();
        let err = s
            .advance(&Frame::Final {
                ref_id: "r-wrong".into(),
                receipt: json!({}),
            })
            .unwrap_err();
        assert!(err.to_string().contains("ref_id mismatch"));
    }

    #[test]
    fn mismatched_fatal_ref_id_rejected() {
        let mut s = ProtocolState::new();
        s.advance(&hello_frame("b")).unwrap();
        s.advance(&Frame::Run {
            id: "r1".into(),
            work_order: json!({}),
        })
        .unwrap();
        let err = s.advance(&fatal_frame(Some("r-wrong"), "err")).unwrap_err();
        assert!(err.to_string().contains("ref_id mismatch"));
    }

    #[test]
    fn fatal_with_none_ref_id_accepted_during_streaming() {
        let mut s = ProtocolState::new();
        s.advance(&hello_frame("b")).unwrap();
        s.advance(&Frame::Run {
            id: "r1".into(),
            work_order: json!({}),
        })
        .unwrap();
        s.advance(&fatal_frame(None, "crash")).unwrap();
        assert_eq!(s.phase(), ProtocolPhase::Completed);
    }

    #[test]
    fn multiple_events_with_correct_ref_id() {
        let mut s = ProtocolState::new();
        s.advance(&hello_frame("b")).unwrap();
        s.advance(&Frame::Run {
            id: "my-run".into(),
            work_order: json!({}),
        })
        .unwrap();
        for _ in 0..10 {
            s.advance(&event_frame("my-run", json!({}))).unwrap();
        }
        assert_eq!(s.events_seen(), 10);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: edge_cases (~12 tests)
// ═══════════════════════════════════════════════════════════════════════
mod edge_cases {
    use super::*;

    #[test]
    fn empty_line_handling_reader() {
        let data = b"\n\n\n";
        let mut r = FrameReader::new(BufReader::new(data.as_slice()));
        assert!(r.read_frame().unwrap().is_none());
    }

    #[test]
    fn whitespace_only_lines_skipped() {
        let data = b"   \n  \t  \n";
        let mut r = FrameReader::new(BufReader::new(data.as_slice()));
        assert!(r.read_frame().unwrap().is_none());
    }

    #[test]
    fn invalid_json_error() {
        let data = b"{not json}\n";
        let mut r = FrameReader::new(BufReader::new(data.as_slice()));
        assert!(r.read_frame().is_err());
    }

    #[test]
    fn valid_json_but_wrong_structure() {
        let data = br#"{"foo": "bar"}"#.to_vec();
        let mut data_with_nl = data;
        data_with_nl.push(b'\n');
        let mut r = FrameReader::new(BufReader::new(data_with_nl.as_slice()));
        assert!(r.read_frame().is_err());
    }

    #[test]
    fn missing_t_tag_fails() {
        let data = br#"{"type":"hello","contract_version":"abp/v0.1"}"#.to_vec();
        let mut data_with_nl = data;
        data_with_nl.push(b'\n');
        let mut r = FrameReader::new(BufReader::new(data_with_nl.as_slice()));
        assert!(r.read_frame().is_err());
    }

    #[test]
    fn utf8_content_roundtrip() {
        let frame = Frame::Fatal {
            ref_id: None,
            error: "エラー: 失敗しました 🔥".into(),
        };
        let mut buf = Vec::new();
        let mut w = FrameWriter::new(&mut buf);
        w.write_frame(&frame).unwrap();
        let mut r = FrameReader::new(BufReader::new(buf.as_slice()));
        let out = r.read_frame().unwrap().unwrap();
        match out {
            Frame::Fatal { error, .. } => assert_eq!(error, "エラー: 失敗しました 🔥"),
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn emoji_in_event_data() {
        let frame = event_frame("r1", json!({"text": "Hello 👋 World 🌍"}));
        let json = frame_to_json(&frame).unwrap();
        let decoded = json_to_frame(&json).unwrap();
        match decoded {
            Frame::Event { event, .. } => {
                assert_eq!(event["text"], "Hello 👋 World 🌍");
            }
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn frame_to_json_and_back() {
        let frame = Frame::Ping { seq: 999 };
        let json = frame_to_json(&frame).unwrap();
        let decoded = json_to_frame(&json).unwrap();
        match decoded {
            Frame::Ping { seq } => assert_eq!(seq, 999),
            _ => panic!("expected Ping"),
        }
    }

    #[test]
    fn concurrent_frame_writing() {
        use std::sync::{Arc, Mutex};
        let buf = Arc::new(Mutex::new(Vec::new()));
        let handles: Vec<_> = (0..10)
            .map(|i| {
                let buf = Arc::clone(&buf);
                std::thread::spawn(move || {
                    let frame = event_frame(&format!("r-{i}"), json!({"i": i}));
                    let encoded = JsonlCodec::encode(&frame).unwrap();
                    let mut guard = buf.lock().unwrap();
                    guard.extend_from_slice(encoded.as_bytes());
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let data = buf.lock().unwrap();
        let lines: Vec<&str> = std::str::from_utf8(&data)
            .unwrap()
            .lines()
            .filter(|l| !l.trim().is_empty())
            .collect();
        assert_eq!(lines.len(), 10);
    }

    #[test]
    fn very_large_payload_in_event() {
        let big_text = "x".repeat(1024);
        let frame = event_frame("r1", json!({"text": big_text}));
        let mut buf = Vec::new();
        let mut w = FrameWriter::new(&mut buf);
        w.write_frame(&frame).unwrap();
        let mut r = FrameReader::new(BufReader::new(buf.as_slice()));
        let out = r.read_frame().unwrap().unwrap();
        match out {
            Frame::Event { event, .. } => {
                assert_eq!(event["text"].as_str().unwrap().len(), 1024);
            }
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn newline_in_json_string_value() {
        let frame = Frame::Fatal {
            ref_id: None,
            error: "line1\nline2".into(),
        };
        let mut buf = Vec::new();
        let mut w = FrameWriter::new(&mut buf);
        w.write_frame(&frame).unwrap();
        let output = String::from_utf8(buf.clone()).unwrap();
        // JSON encodes \n as \\n, so there should be exactly one line.
        assert_eq!(output.lines().count(), 1);
        let mut r = FrameReader::new(BufReader::new(buf.as_slice()));
        let out = r.read_frame().unwrap().unwrap();
        match out {
            Frame::Fatal { error, .. } => assert_eq!(error, "line1\nline2"),
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn default_max_frame_size_is_16mib() {
        assert_eq!(DEFAULT_MAX_FRAME_SIZE, 16 * 1024 * 1024);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: envelope_validation_via_json (~8 tests)
// ═══════════════════════════════════════════════════════════════════════
mod envelope_validation {
    use super::*;

    #[test]
    fn tag_field_is_t_not_type() {
        let frame = hello_frame("b");
        let json = frame_to_json(&frame).unwrap();
        let parsed: Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.get("t").is_some());
        assert!(parsed.get("type").is_none());
    }

    #[test]
    fn all_variants_use_t_tag() {
        let frames: Vec<Frame> = vec![
            hello_frame("b"),
            Frame::Run {
                id: "r".into(),
                work_order: json!({}),
            },
            event_frame("r", json!({})),
            Frame::Final {
                ref_id: "r".into(),
                receipt: json!({}),
            },
            fatal_frame(None, "e"),
            Frame::Cancel {
                ref_id: "r".into(),
                reason: None,
            },
            Frame::Ping { seq: 0 },
            Frame::Pong { seq: 0 },
        ];
        for frame in &frames {
            let json = frame_to_json(frame).unwrap();
            let parsed: Value = serde_json::from_str(&json).unwrap();
            assert!(parsed.get("t").is_some(), "frame missing 't' tag: {json}");
        }
    }

    #[test]
    fn hello_tag_value_is_hello() {
        let json = frame_to_json(&hello_frame("b")).unwrap();
        let parsed: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["t"], "hello");
    }

    #[test]
    fn run_tag_value_is_run() {
        let json = frame_to_json(&Frame::Run {
            id: "r".into(),
            work_order: json!({}),
        })
        .unwrap();
        let parsed: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["t"], "run");
    }

    #[test]
    fn event_tag_value_is_event() {
        let json = frame_to_json(&event_frame("r", json!({}))).unwrap();
        let parsed: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["t"], "event");
    }

    #[test]
    fn final_tag_value_is_final() {
        let json = frame_to_json(&Frame::Final {
            ref_id: "r".into(),
            receipt: json!({}),
        })
        .unwrap();
        let parsed: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["t"], "final");
    }

    #[test]
    fn fatal_tag_value_is_fatal() {
        let json = frame_to_json(&fatal_frame(None, "e")).unwrap();
        let parsed: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["t"], "fatal");
    }

    #[test]
    fn unknown_tag_value_rejected() {
        let raw = r#"{"t":"unknown_variant","data":"x"}"#;
        let result = json_to_frame(raw);
        assert!(result.is_err());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: full protocol walkthrough (~4 tests)
// ═══════════════════════════════════════════════════════════════════════
mod full_protocol {
    use super::*;

    #[test]
    fn happy_path_hello_run_events_final() {
        let mut s = ProtocolState::new();
        let frames = vec![
            hello_frame("b"),
            Frame::Run {
                id: "r1".into(),
                work_order: json!({"task": "test"}),
            },
            event_frame("r1", event_text_delta("hi")),
            event_frame("r1", event_text_delta("there")),
            Frame::Final {
                ref_id: "r1".into(),
                receipt: json!({"outcome": "complete"}),
            },
        ];

        // Write all frames then read them back.
        let mut buf = Vec::new();
        write_frames(&mut buf, &frames).unwrap();
        let read_back = read_all_frames(BufReader::new(buf.as_slice())).unwrap();
        assert_eq!(read_back.len(), 5);

        // Feed all through state machine.
        for f in &read_back {
            s.advance(f).unwrap();
        }
        assert_eq!(s.phase(), ProtocolPhase::Completed);
        assert_eq!(s.events_seen(), 2);
    }

    #[test]
    fn happy_path_with_fatal() {
        let mut s = ProtocolState::new();
        s.advance(&hello_frame("b")).unwrap();
        s.advance(&Frame::Run {
            id: "r1".into(),
            work_order: json!({}),
        })
        .unwrap();
        s.advance(&event_frame("r1", json!({}))).unwrap();
        s.advance(&fatal_frame(Some("r1"), "oom")).unwrap();
        assert_eq!(s.phase(), ProtocolPhase::Completed);
        assert_eq!(s.events_seen(), 1);
    }

    #[test]
    fn reset_allows_new_session() {
        let mut s = ProtocolState::new();
        s.advance(&hello_frame("b")).unwrap();
        s.advance(&Frame::Run {
            id: "r1".into(),
            work_order: json!({}),
        })
        .unwrap();
        s.advance(&Frame::Final {
            ref_id: "r1".into(),
            receipt: json!({}),
        })
        .unwrap();
        assert!(s.is_terminal());

        s.reset();
        assert_eq!(s.phase(), ProtocolPhase::AwaitingHello);
        s.advance(&hello_frame("b2")).unwrap();
        assert_eq!(s.phase(), ProtocolPhase::AwaitingRun);
    }

    #[test]
    fn write_read_validate_integrated() {
        let frames = vec![
            hello_frame("backend"),
            Frame::Run {
                id: "r1".into(),
                work_order: json!({"task": "foo"}),
            },
            event_frame("r1", event_text_delta("output")),
            Frame::Final {
                ref_id: "r1".into(),
                receipt: json!({"outcome": "complete"}),
            },
        ];

        // Validate every frame.
        for f in &frames {
            let v = validate_frame(f, DEFAULT_MAX_FRAME_SIZE);
            assert!(v.valid, "frame failed validation: {:?}", v.issues);
        }

        // Write, read, and verify through state machine.
        let mut buf = Vec::new();
        write_frames(&mut buf, &frames).unwrap();
        let read_back = read_all_frames(BufReader::new(buf.as_slice())).unwrap();

        let mut s = ProtocolState::new();
        for f in &read_back {
            s.advance(f).unwrap();
        }
        assert_eq!(s.phase(), ProtocolPhase::Completed);
    }
}
