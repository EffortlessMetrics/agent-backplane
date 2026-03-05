// SPDX-License-Identifier: MIT OR Apache-2.0
//! Exhaustive tests for the ABP sidecar protocol state machine.
//!
//! Covers valid transitions, invalid transitions, error recovery,
//! protocol edge cases, and concurrent/multi-run scenarios.

use serde_json::{json, Value};
use sidecar_kit::{
    builders::{
        event_command_executed, event_error, event_file_changed, event_frame, event_run_completed,
        event_run_started, event_text_delta, event_text_message, event_tool_call,
        event_tool_result, event_warning, fatal_frame, final_frame, hello_frame, EventBuilder,
        ReceiptBuilder,
    },
    codec::JsonlCodec,
    framing::{read_all_frames, validate_frame, write_frames, FrameReader, FrameWriter},
    test_utils::MockStdin,
    Frame, ProtocolPhase, ProtocolState,
};

// ═══════════════════════════════════════════════════════════════════════
// Helper functions
// ═══════════════════════════════════════════════════════════════════════

fn make_hello() -> Frame {
    hello_frame("test-backend")
}

fn make_run(id: &str) -> Frame {
    Frame::Run {
        id: id.to_string(),
        work_order: json!({"task": "test task"}),
    }
}

fn make_event(ref_id: &str) -> Frame {
    event_frame(ref_id, event_text_delta("hello world"))
}

fn make_final(ref_id: &str) -> Frame {
    final_frame(ref_id, ReceiptBuilder::new(ref_id, "test-backend").build())
}

fn make_fatal(ref_id: Option<&str>) -> Frame {
    fatal_frame(ref_id, "something went wrong")
}

fn make_ping(seq: u64) -> Frame {
    Frame::Ping { seq }
}

fn make_pong(seq: u64) -> Frame {
    Frame::Pong { seq }
}

fn make_cancel(ref_id: &str) -> Frame {
    Frame::Cancel {
        ref_id: ref_id.to_string(),
        reason: Some("user requested".to_string()),
    }
}

/// Advance through hello and run to reach Streaming phase.
fn advance_to_streaming(state: &mut ProtocolState, run_id: &str) {
    state.advance(&make_hello()).unwrap();
    state.advance(&make_run(run_id)).unwrap();
    assert_eq!(state.phase(), ProtocolPhase::Streaming);
}

/// Advance through the full happy path to Completed phase.
fn advance_to_completed(state: &mut ProtocolState, run_id: &str) {
    advance_to_streaming(state, run_id);
    state.advance(&make_event(run_id)).unwrap();
    state.advance(&make_final(run_id)).unwrap();
    assert_eq!(state.phase(), ProtocolPhase::Completed);
}

// ═══════════════════════════════════════════════════════════════════════
// 1. VALID TRANSITIONS (20 tests)
// ═══════════════════════════════════════════════════════════════════════

mod valid_transitions {
    use super::*;

    #[test]
    fn hello_transitions_to_awaiting_run() {
        let mut state = ProtocolState::new();
        assert_eq!(state.phase(), ProtocolPhase::AwaitingHello);
        state.advance(&make_hello()).unwrap();
        assert_eq!(state.phase(), ProtocolPhase::AwaitingRun);
    }

    #[test]
    fn run_transitions_to_streaming() {
        let mut state = ProtocolState::new();
        state.advance(&make_hello()).unwrap();
        state.advance(&make_run("run-1")).unwrap();
        assert_eq!(state.phase(), ProtocolPhase::Streaming);
        assert_eq!(state.run_id(), Some("run-1"));
    }

    #[test]
    fn event_stays_in_streaming() {
        let mut state = ProtocolState::new();
        advance_to_streaming(&mut state, "run-1");
        state.advance(&make_event("run-1")).unwrap();
        assert_eq!(state.phase(), ProtocolPhase::Streaming);
        assert_eq!(state.events_seen(), 1);
    }

    #[test]
    fn final_transitions_to_completed() {
        let mut state = ProtocolState::new();
        advance_to_streaming(&mut state, "run-1");
        state.advance(&make_final("run-1")).unwrap();
        assert_eq!(state.phase(), ProtocolPhase::Completed);
    }

    #[test]
    fn fatal_from_streaming_transitions_to_completed() {
        let mut state = ProtocolState::new();
        advance_to_streaming(&mut state, "run-1");
        state.advance(&make_fatal(Some("run-1"))).unwrap();
        assert_eq!(state.phase(), ProtocolPhase::Completed);
    }

    #[test]
    fn fatal_from_awaiting_run_transitions_to_completed() {
        let mut state = ProtocolState::new();
        state.advance(&make_hello()).unwrap();
        state.advance(&make_fatal(None)).unwrap();
        assert_eq!(state.phase(), ProtocolPhase::Completed);
    }

    #[test]
    fn hello_run_event_final_full_happy_path() {
        let mut state = ProtocolState::new();
        state.advance(&make_hello()).unwrap();
        state.advance(&make_run("run-1")).unwrap();
        state.advance(&make_event("run-1")).unwrap();
        state.advance(&make_final("run-1")).unwrap();
        assert_eq!(state.phase(), ProtocolPhase::Completed);
        assert!(state.is_terminal());
    }

    #[test]
    fn hello_run_fatal_short_path() {
        let mut state = ProtocolState::new();
        state.advance(&make_hello()).unwrap();
        state.advance(&make_run("run-1")).unwrap();
        state
            .advance(&fatal_frame(Some("run-1"), "backend crashed"))
            .unwrap();
        assert_eq!(state.phase(), ProtocolPhase::Completed);
    }

    #[test]
    fn hello_run_multiple_events_then_final() {
        let mut state = ProtocolState::new();
        advance_to_streaming(&mut state, "run-1");
        for i in 0..10 {
            state
                .advance(&event_frame(
                    "run-1",
                    event_text_delta(&format!("chunk {i}")),
                ))
                .unwrap();
        }
        assert_eq!(state.events_seen(), 10);
        state.advance(&make_final("run-1")).unwrap();
        assert_eq!(state.phase(), ProtocolPhase::Completed);
    }

    #[test]
    fn hello_run_multiple_events_then_fatal() {
        let mut state = ProtocolState::new();
        advance_to_streaming(&mut state, "run-1");
        state.advance(&make_event("run-1")).unwrap();
        state.advance(&make_event("run-1")).unwrap();
        state.advance(&make_event("run-1")).unwrap();
        state.advance(&make_fatal(Some("run-1"))).unwrap();
        assert_eq!(state.phase(), ProtocolPhase::Completed);
        assert_eq!(state.events_seen(), 3);
    }

    #[test]
    fn events_seen_counter_tracks_correctly() {
        let mut state = ProtocolState::new();
        advance_to_streaming(&mut state, "run-1");
        assert_eq!(state.events_seen(), 0);

        for expected in 1..=5 {
            state.advance(&make_event("run-1")).unwrap();
            assert_eq!(state.events_seen(), expected);
        }
    }

    #[test]
    fn run_id_is_none_before_run() {
        let mut state = ProtocolState::new();
        assert!(state.run_id().is_none());
        state.advance(&make_hello()).unwrap();
        assert!(state.run_id().is_none());
    }

    #[test]
    fn run_id_set_after_run() {
        let mut state = ProtocolState::new();
        state.advance(&make_hello()).unwrap();
        state.advance(&make_run("my-unique-run")).unwrap();
        assert_eq!(state.run_id(), Some("my-unique-run"));
    }

    #[test]
    fn is_terminal_false_before_completion() {
        let mut state = ProtocolState::new();
        assert!(!state.is_terminal());
        state.advance(&make_hello()).unwrap();
        assert!(!state.is_terminal());
        state.advance(&make_run("run-1")).unwrap();
        assert!(!state.is_terminal());
    }

    #[test]
    fn is_terminal_true_after_final() {
        let mut state = ProtocolState::new();
        advance_to_completed(&mut state, "run-1");
        assert!(state.is_terminal());
    }

    #[test]
    fn is_terminal_true_after_fatal() {
        let mut state = ProtocolState::new();
        advance_to_streaming(&mut state, "run-1");
        state.advance(&make_fatal(Some("run-1"))).unwrap();
        assert!(state.is_terminal());
    }

    #[test]
    fn ping_allowed_during_streaming() {
        let mut state = ProtocolState::new();
        advance_to_streaming(&mut state, "run-1");
        state.advance(&make_ping(1)).unwrap();
        assert_eq!(state.phase(), ProtocolPhase::Streaming);
        // Pings don't count as events
        assert_eq!(state.events_seen(), 0);
    }

    #[test]
    fn pong_allowed_during_streaming() {
        let mut state = ProtocolState::new();
        advance_to_streaming(&mut state, "run-1");
        state.advance(&make_pong(1)).unwrap();
        assert_eq!(state.phase(), ProtocolPhase::Streaming);
    }

    #[test]
    fn interleaved_ping_pong_with_events() {
        let mut state = ProtocolState::new();
        advance_to_streaming(&mut state, "run-1");
        state.advance(&make_ping(1)).unwrap();
        state.advance(&make_event("run-1")).unwrap();
        state.advance(&make_pong(1)).unwrap();
        state.advance(&make_event("run-1")).unwrap();
        state.advance(&make_ping(2)).unwrap();
        state.advance(&make_final("run-1")).unwrap();
        assert_eq!(state.phase(), ProtocolPhase::Completed);
        assert_eq!(state.events_seen(), 2);
    }

    #[test]
    fn hello_with_custom_contract_version() {
        let mut state = ProtocolState::new();
        let hello = Frame::Hello {
            contract_version: "abp/v0.2".to_string(),
            backend: json!({"id": "custom"}),
            capabilities: json!({"streaming": true}),
            mode: json!("passthrough"),
        };
        state.advance(&hello).unwrap();
        assert_eq!(state.phase(), ProtocolPhase::AwaitingRun);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 2. INVALID TRANSITIONS (15 tests)
// ═══════════════════════════════════════════════════════════════════════

mod invalid_transitions {
    use super::*;

    #[test]
    fn run_before_hello_faults() {
        let mut state = ProtocolState::new();
        let err = state.advance(&make_run("run-1")).unwrap_err();
        assert_eq!(state.phase(), ProtocolPhase::Faulted);
        assert!(err.to_string().contains("expected hello"), "error: {err}");
    }

    #[test]
    fn event_before_hello_faults() {
        let mut state = ProtocolState::new();
        let err = state.advance(&make_event("run-1")).unwrap_err();
        assert_eq!(state.phase(), ProtocolPhase::Faulted);
        assert!(err.to_string().contains("expected hello"));
    }

    #[test]
    fn final_before_hello_faults() {
        let mut state = ProtocolState::new();
        let err = state.advance(&make_final("run-1")).unwrap_err();
        assert_eq!(state.phase(), ProtocolPhase::Faulted);
        assert!(err.to_string().contains("expected hello"));
    }

    #[test]
    fn fatal_before_hello_faults() {
        let mut state = ProtocolState::new();
        let err = state.advance(&make_fatal(Some("run-1"))).unwrap_err();
        assert_eq!(state.phase(), ProtocolPhase::Faulted);
        assert!(err.to_string().contains("expected hello"));
    }

    #[test]
    fn double_hello_faults() {
        let mut state = ProtocolState::new();
        state.advance(&make_hello()).unwrap();
        let err = state.advance(&make_hello()).unwrap_err();
        assert_eq!(state.phase(), ProtocolPhase::Faulted);
        assert!(err.to_string().contains("expected run or fatal, got hello"));
    }

    #[test]
    fn event_before_run_faults() {
        let mut state = ProtocolState::new();
        state.advance(&make_hello()).unwrap();
        let err = state.advance(&make_event("run-1")).unwrap_err();
        assert_eq!(state.phase(), ProtocolPhase::Faulted);
        assert!(err.to_string().contains("expected run or fatal"));
    }

    #[test]
    fn final_before_run_faults() {
        let mut state = ProtocolState::new();
        state.advance(&make_hello()).unwrap();
        let _err = state.advance(&make_final("run-1")).unwrap_err();
        assert_eq!(state.phase(), ProtocolPhase::Faulted);
    }

    #[test]
    fn event_after_final_faults() {
        let mut state = ProtocolState::new();
        advance_to_completed(&mut state, "run-1");
        let err = state.advance(&make_event("run-1")).unwrap_err();
        assert_eq!(state.phase(), ProtocolPhase::Faulted);
        assert!(err.to_string().contains("protocol already completed"));
    }

    #[test]
    fn run_after_final_faults() {
        let mut state = ProtocolState::new();
        advance_to_completed(&mut state, "run-1");
        let err = state.advance(&make_run("run-2")).unwrap_err();
        assert_eq!(state.phase(), ProtocolPhase::Faulted);
        assert!(err.to_string().contains("protocol already completed"));
    }

    #[test]
    fn hello_after_final_faults() {
        let mut state = ProtocolState::new();
        advance_to_completed(&mut state, "run-1");
        let _err = state.advance(&make_hello()).unwrap_err();
        assert_eq!(state.phase(), ProtocolPhase::Faulted);
    }

    #[test]
    fn double_final_faults() {
        let mut state = ProtocolState::new();
        advance_to_streaming(&mut state, "run-1");
        state.advance(&make_final("run-1")).unwrap();
        let _err = state.advance(&make_final("run-1")).unwrap_err();
        assert_eq!(state.phase(), ProtocolPhase::Faulted);
    }

    #[test]
    fn fatal_after_final_faults() {
        let mut state = ProtocolState::new();
        advance_to_completed(&mut state, "run-1");
        let _err = state.advance(&make_fatal(Some("run-1"))).unwrap_err();
        assert_eq!(state.phase(), ProtocolPhase::Faulted);
    }

    #[test]
    fn ref_id_mismatch_during_event() {
        let mut state = ProtocolState::new();
        advance_to_streaming(&mut state, "run-1");
        let err = state.advance(&make_event("wrong-id")).unwrap_err();
        assert!(err.to_string().contains("ref_id mismatch"));
    }

    #[test]
    fn ref_id_mismatch_during_final() {
        let mut state = ProtocolState::new();
        advance_to_streaming(&mut state, "run-1");
        let err = state.advance(&make_final("wrong-id")).unwrap_err();
        assert!(err.to_string().contains("ref_id mismatch"));
    }

    #[test]
    fn cancel_during_streaming_faults() {
        let mut state = ProtocolState::new();
        advance_to_streaming(&mut state, "run-1");
        // Cancel is not in the valid set for Streaming in the state machine
        let _err = state.advance(&make_cancel("run-1")).unwrap_err();
        assert_eq!(state.phase(), ProtocolPhase::Faulted);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 3. ERROR RECOVERY (10 tests)
// ═══════════════════════════════════════════════════════════════════════

mod error_recovery {
    use super::*;

    #[test]
    fn faulted_state_rejects_all_frames() {
        let mut state = ProtocolState::new();
        // Fault the machine
        let _ = state.advance(&make_run("run-1"));
        assert_eq!(state.phase(), ProtocolPhase::Faulted);

        // All further frames should be rejected
        assert!(state.advance(&make_hello()).is_err());
        assert!(state.advance(&make_run("run-1")).is_err());
        assert!(state.advance(&make_event("run-1")).is_err());
        assert!(state.advance(&make_final("run-1")).is_err());
        assert!(state.advance(&make_fatal(None)).is_err());
    }

    #[test]
    fn faulted_error_message_mentions_reset() {
        let mut state = ProtocolState::new();
        let _ = state.advance(&make_event("run-1"));
        let err = state.advance(&make_hello()).unwrap_err();
        assert!(
            err.to_string().contains("faulted") && err.to_string().contains("reset"),
            "error should mention faulted state and reset: {err}"
        );
    }

    #[test]
    fn reset_from_faulted_restores_initial_state() {
        let mut state = ProtocolState::new();
        let _ = state.advance(&make_run("run-1"));
        assert_eq!(state.phase(), ProtocolPhase::Faulted);

        state.reset();
        assert_eq!(state.phase(), ProtocolPhase::AwaitingHello);
        assert!(state.run_id().is_none());
        assert_eq!(state.events_seen(), 0);
        assert!(state.fault_reason().is_none());
    }

    #[test]
    fn reset_after_completed_allows_new_session() {
        let mut state = ProtocolState::new();
        advance_to_completed(&mut state, "run-1");
        assert!(state.is_terminal());

        state.reset();
        assert_eq!(state.phase(), ProtocolPhase::AwaitingHello);
        // Can start fresh
        state.advance(&make_hello()).unwrap();
        state.advance(&make_run("run-2")).unwrap();
        assert_eq!(state.phase(), ProtocolPhase::Streaming);
        assert_eq!(state.run_id(), Some("run-2"));
    }

    #[test]
    fn fault_reason_is_captured() {
        let mut state = ProtocolState::new();
        let _ = state.advance(&make_event("run-1"));
        assert!(state.fault_reason().is_some());
        assert!(
            state.fault_reason().unwrap().contains("expected hello"),
            "fault reason: {:?}",
            state.fault_reason()
        );
    }

    #[test]
    fn fault_reason_cleared_on_reset() {
        let mut state = ProtocolState::new();
        let _ = state.advance(&make_run("x"));
        assert!(state.fault_reason().is_some());
        state.reset();
        assert!(state.fault_reason().is_none());
    }

    #[test]
    fn fatal_with_error_events_completes_normally() {
        let mut state = ProtocolState::new();
        advance_to_streaming(&mut state, "run-1");
        // Error events are still valid events
        state
            .advance(&event_frame("run-1", event_error("something failed")))
            .unwrap();
        state
            .advance(&event_frame("run-1", event_error("another failure")))
            .unwrap();
        // Fatal terminates
        state.advance(&make_fatal(Some("run-1"))).unwrap();
        assert_eq!(state.phase(), ProtocolPhase::Completed);
        assert_eq!(state.events_seen(), 2);
    }

    #[test]
    fn recovery_after_error_events_via_final() {
        let mut state = ProtocolState::new();
        advance_to_streaming(&mut state, "run-1");
        // Error events don't prevent successful completion
        state
            .advance(&event_frame("run-1", event_error("recoverable error")))
            .unwrap();
        state
            .advance(&event_frame("run-1", event_warning("just a warning")))
            .unwrap();
        state
            .advance(&event_frame("run-1", event_text_delta("recovered output")))
            .unwrap();
        state.advance(&make_final("run-1")).unwrap();
        assert_eq!(state.phase(), ProtocolPhase::Completed);
        assert_eq!(state.events_seen(), 3);
    }

    #[test]
    fn reset_from_streaming_clears_state() {
        let mut state = ProtocolState::new();
        advance_to_streaming(&mut state, "run-1");
        state.advance(&make_event("run-1")).unwrap();
        assert_eq!(state.events_seen(), 1);

        state.reset();
        assert_eq!(state.phase(), ProtocolPhase::AwaitingHello);
        assert_eq!(state.events_seen(), 0);
        assert!(state.run_id().is_none());
    }

    #[test]
    fn multiple_reset_cycles() {
        let mut state = ProtocolState::new();

        // Cycle 1: complete normally
        advance_to_completed(&mut state, "run-1");
        state.reset();

        // Cycle 2: fault and recover
        let _ = state.advance(&make_run("x"));
        assert_eq!(state.phase(), ProtocolPhase::Faulted);
        state.reset();

        // Cycle 3: complete via fatal
        state.advance(&make_hello()).unwrap();
        state.advance(&make_run("run-3")).unwrap();
        state.advance(&make_fatal(Some("run-3"))).unwrap();
        assert_eq!(state.phase(), ProtocolPhase::Completed);

        state.reset();
        assert_eq!(state.phase(), ProtocolPhase::AwaitingHello);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 4. PROTOCOL EDGE CASES (15 tests)
// ═══════════════════════════════════════════════════════════════════════

mod edge_cases {
    use super::*;

    #[test]
    fn empty_event_stream_hello_run_final() {
        let mut state = ProtocolState::new();
        state.advance(&make_hello()).unwrap();
        state.advance(&make_run("run-1")).unwrap();
        // No events at all, go straight to final
        state.advance(&make_final("run-1")).unwrap();
        assert_eq!(state.phase(), ProtocolPhase::Completed);
        assert_eq!(state.events_seen(), 0);
    }

    #[test]
    fn large_event_payload_accepted() {
        let mut state = ProtocolState::new();
        advance_to_streaming(&mut state, "run-1");
        let large_text = "x".repeat(1_000_000);
        let event = event_frame("run-1", event_text_message(&large_text));
        state.advance(&event).unwrap();
        assert_eq!(state.events_seen(), 1);
    }

    #[test]
    fn unicode_in_event_payload() {
        let mut state = ProtocolState::new();
        advance_to_streaming(&mut state, "run-1");
        let event = event_frame(
            "run-1",
            event_text_delta("日本語テスト 🦀 émojis and ñ special chars Ω"),
        );
        state.advance(&event).unwrap();
        assert_eq!(state.events_seen(), 1);
    }

    #[test]
    fn unicode_in_run_id() {
        let mut state = ProtocolState::new();
        state.advance(&make_hello()).unwrap();
        let run_id = "run-日本語-🦀";
        state.advance(&make_run(run_id)).unwrap();
        assert_eq!(state.run_id(), Some(run_id));
        state.advance(&make_event(run_id)).unwrap();
        state.advance(&make_final(run_id)).unwrap();
        assert_eq!(state.phase(), ProtocolPhase::Completed);
    }

    #[test]
    fn null_fields_in_event_value() {
        let mut state = ProtocolState::new();
        advance_to_streaming(&mut state, "run-1");
        let event = event_frame(
            "run-1",
            json!({"ts": null, "type": "assistant_delta", "text": null}),
        );
        state.advance(&event).unwrap();
        assert_eq!(state.events_seen(), 1);
    }

    #[test]
    fn empty_string_run_id() {
        let mut state = ProtocolState::new();
        state.advance(&make_hello()).unwrap();
        // The state machine itself doesn't validate run_id content,
        // it stores whatever is given
        state.advance(&make_run("")).unwrap();
        assert_eq!(state.run_id(), Some(""));
        assert_eq!(state.phase(), ProtocolPhase::Streaming);
    }

    #[test]
    fn fatal_with_no_ref_id_during_streaming() {
        let mut state = ProtocolState::new();
        advance_to_streaming(&mut state, "run-1");
        // Fatal without ref_id is valid during streaming
        state.advance(&make_fatal(None)).unwrap();
        assert_eq!(state.phase(), ProtocolPhase::Completed);
    }

    #[test]
    fn fatal_with_mismatched_ref_id_during_streaming() {
        let mut state = ProtocolState::new();
        advance_to_streaming(&mut state, "run-1");
        // Fatal with wrong ref_id should fail ref_id check
        let result = state.advance(&make_fatal(Some("wrong-run")));
        assert!(result.is_err());
    }

    #[test]
    fn many_different_event_types() {
        let mut state = ProtocolState::new();
        advance_to_streaming(&mut state, "run-1");

        let events = [
            event_frame("run-1", event_run_started("starting")),
            event_frame("run-1", event_text_delta("chunk 1")),
            event_frame("run-1", event_text_message("full message")),
            event_frame(
                "run-1",
                event_tool_call("read_file", Some("tc-1"), json!({"path": "test.rs"})),
            ),
            event_frame(
                "run-1",
                event_tool_result("read_file", Some("tc-1"), json!("contents"), false),
            ),
            event_frame("run-1", event_file_changed("src/main.rs", "added function")),
            event_frame(
                "run-1",
                event_command_executed("cargo build", Some(0), Some("ok")),
            ),
            event_frame("run-1", event_warning("something odd")),
            event_frame("run-1", event_error("non-fatal error")),
            event_frame("run-1", event_run_completed("done")),
        ];

        for e in &events {
            state.advance(e).unwrap();
        }
        assert_eq!(state.events_seen(), 10);
        state.advance(&make_final("run-1")).unwrap();
        assert_eq!(state.phase(), ProtocolPhase::Completed);
    }

    #[test]
    fn event_builder_custom_fields() {
        let mut state = ProtocolState::new();
        advance_to_streaming(&mut state, "run-1");
        let event = EventBuilder::new("custom_event")
            .field("custom_field", json!([1, 2, 3]))
            .field("nested", json!({"a": {"b": "c"}}))
            .message("custom message")
            .build();
        state.advance(&event_frame("run-1", event)).unwrap();
        assert_eq!(state.events_seen(), 1);
    }

    #[test]
    fn receipt_builder_with_all_fields() {
        let mut state = ProtocolState::new();
        advance_to_streaming(&mut state, "run-1");

        let receipt = ReceiptBuilder::new("run-1", "test-backend")
            .event(event_text_delta("hi"))
            .event(event_run_completed("done"))
            .artifact("patch", "output.diff")
            .usage_raw(json!({"model": "test", "tokens": 100}))
            .input_tokens(50)
            .output_tokens(50)
            .build();

        state.advance(&final_frame("run-1", receipt)).unwrap();
        assert_eq!(state.phase(), ProtocolPhase::Completed);
    }

    #[test]
    fn receipt_builder_failed_outcome() {
        let mut state = ProtocolState::new();
        advance_to_streaming(&mut state, "run-1");

        let receipt = ReceiptBuilder::new("run-1", "test-backend")
            .failed()
            .build();
        let receipt_value = &receipt;
        assert_eq!(receipt_value["outcome"], "failed");

        state.advance(&final_frame("run-1", receipt)).unwrap();
        assert_eq!(state.phase(), ProtocolPhase::Completed);
    }

    #[test]
    fn receipt_builder_partial_outcome() {
        let mut state = ProtocolState::new();
        advance_to_streaming(&mut state, "run-1");

        let receipt = ReceiptBuilder::new("run-1", "test-backend")
            .partial()
            .build();
        assert_eq!(receipt["outcome"], "partial");

        state.advance(&final_frame("run-1", receipt)).unwrap();
        assert_eq!(state.phase(), ProtocolPhase::Completed);
    }

    #[test]
    fn default_state_is_awaiting_hello() {
        let state = ProtocolState::default();
        assert_eq!(state.phase(), ProtocolPhase::AwaitingHello);
        assert!(state.run_id().is_none());
        assert_eq!(state.events_seen(), 0);
        assert!(state.fault_reason().is_none());
        assert!(!state.is_terminal());
    }

    #[test]
    fn deeply_nested_event_payload() {
        let mut state = ProtocolState::new();
        advance_to_streaming(&mut state, "run-1");

        // Build a deeply nested JSON value
        let mut nested = json!("leaf");
        for _ in 0..50 {
            nested = json!({"inner": nested});
        }
        let event = event_frame(
            "run-1",
            json!({
                "ts": "2025-01-01T00:00:00Z",
                "type": "tool_result",
                "tool_name": "deep_tool",
                "output": nested,
                "is_error": false,
            }),
        );
        state.advance(&event).unwrap();
        assert_eq!(state.events_seen(), 1);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 5. CONCURRENT / MULTI-RUN SCENARIOS (5 tests)
// ═══════════════════════════════════════════════════════════════════════

mod concurrent_protocol {
    use super::*;

    #[test]
    fn sequential_runs_via_reset() {
        let mut state = ProtocolState::new();

        // Run 1
        advance_to_completed(&mut state, "run-1");
        state.reset();

        // Run 2
        state.advance(&make_hello()).unwrap();
        state.advance(&make_run("run-2")).unwrap();
        state.advance(&make_event("run-2")).unwrap();
        state.advance(&make_final("run-2")).unwrap();
        assert_eq!(state.phase(), ProtocolPhase::Completed);
        assert_eq!(state.run_id(), Some("run-2"));
    }

    #[test]
    fn multiple_state_machines_independent() {
        let mut state_a = ProtocolState::new();
        let mut state_b = ProtocolState::new();

        state_a.advance(&make_hello()).unwrap();
        state_b.advance(&make_hello()).unwrap();

        state_a.advance(&make_run("run-a")).unwrap();
        state_b.advance(&make_run("run-b")).unwrap();

        state_a.advance(&make_event("run-a")).unwrap();
        state_b.advance(&make_event("run-b")).unwrap();

        // A completes, B continues
        state_a.advance(&make_final("run-a")).unwrap();
        assert_eq!(state_a.phase(), ProtocolPhase::Completed);
        assert_eq!(state_b.phase(), ProtocolPhase::Streaming);

        state_b.advance(&make_event("run-b")).unwrap();
        state_b.advance(&make_final("run-b")).unwrap();
        assert_eq!(state_b.phase(), ProtocolPhase::Completed);
    }

    #[test]
    fn state_machine_per_run_id_pattern() {
        // Simulate v0.2 multi-run by using separate state machines per run
        let mut machines: std::collections::HashMap<String, ProtocolState> =
            std::collections::HashMap::new();

        for i in 0..5 {
            let run_id = format!("run-{i}");
            let mut state = ProtocolState::new();
            state.advance(&make_hello()).unwrap();
            state.advance(&make_run(&run_id)).unwrap();
            machines.insert(run_id, state);
        }

        // All should be streaming
        for state in machines.values() {
            assert_eq!(state.phase(), ProtocolPhase::Streaming);
        }

        // Complete them in reverse order
        for i in (0..5).rev() {
            let run_id = format!("run-{i}");
            let state = machines.get_mut(&run_id).unwrap();
            state.advance(&make_event(&run_id)).unwrap();
            state.advance(&make_final(&run_id)).unwrap();
            assert_eq!(state.phase(), ProtocolPhase::Completed);
        }
    }

    #[test]
    fn clone_state_machine_diverges() {
        let mut state = ProtocolState::new();
        state.advance(&make_hello()).unwrap();
        state.advance(&make_run("run-1")).unwrap();

        let mut cloned = state.clone();
        assert_eq!(cloned.phase(), ProtocolPhase::Streaming);

        // Original completes normally
        state.advance(&make_final("run-1")).unwrap();
        assert_eq!(state.phase(), ProtocolPhase::Completed);

        // Clone continues streaming independently
        assert_eq!(cloned.phase(), ProtocolPhase::Streaming);
        cloned.advance(&make_event("run-1")).unwrap();
        cloned.advance(&make_fatal(Some("run-1"))).unwrap();
        assert_eq!(cloned.phase(), ProtocolPhase::Completed);
    }

    #[test]
    fn interleaved_events_different_machines() {
        let mut m1 = ProtocolState::new();
        let mut m2 = ProtocolState::new();

        m1.advance(&make_hello()).unwrap();
        m2.advance(&make_hello()).unwrap();
        m1.advance(&make_run("run-1")).unwrap();
        m2.advance(&make_run("run-2")).unwrap();

        // Interleave events
        m1.advance(&make_event("run-1")).unwrap();
        m2.advance(&make_event("run-2")).unwrap();
        m1.advance(&make_event("run-1")).unwrap();
        m2.advance(&make_event("run-2")).unwrap();
        m2.advance(&make_event("run-2")).unwrap();

        assert_eq!(m1.events_seen(), 2);
        assert_eq!(m2.events_seen(), 3);

        m1.advance(&make_final("run-1")).unwrap();
        m2.advance(&make_fatal(Some("run-2"))).unwrap();

        assert_eq!(m1.phase(), ProtocolPhase::Completed);
        assert_eq!(m2.phase(), ProtocolPhase::Completed);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 6. CODEC / WIRE FORMAT INTEGRATION (10 tests)
// ═══════════════════════════════════════════════════════════════════════

mod codec_integration {
    use super::*;

    #[test]
    fn encode_decode_hello_roundtrip() {
        let frame = make_hello();
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        assert!(matches!(decoded, Frame::Hello { .. }));
    }

    #[test]
    fn encode_decode_all_frame_types() {
        let frames = vec![
            make_hello(),
            make_run("run-1"),
            make_event("run-1"),
            make_final("run-1"),
            make_fatal(Some("run-1")),
            make_ping(42),
            make_pong(42),
            make_cancel("run-1"),
        ];

        for frame in &frames {
            let encoded = JsonlCodec::encode(frame).unwrap();
            let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
            // Verify the tag matches
            let orig_json: Value = serde_json::to_value(frame).unwrap();
            let decoded_json: Value = serde_json::to_value(&decoded).unwrap();
            assert_eq!(orig_json["t"], decoded_json["t"]);
        }
    }

    #[test]
    fn frame_writer_reader_roundtrip() {
        let frames = vec![
            make_hello(),
            make_run("run-1"),
            make_event("run-1"),
            make_final("run-1"),
        ];

        let mut buf = Vec::new();
        {
            let mut writer = FrameWriter::new(&mut buf);
            for f in &frames {
                writer.write_frame(f).unwrap();
            }
            writer.flush().unwrap();
            assert_eq!(writer.frames_written(), 4);
        }

        let reader = FrameReader::new(std::io::BufReader::new(buf.as_slice()));
        let read_frames: Vec<Frame> = reader.frames().map(|r| r.unwrap()).collect();
        assert_eq!(read_frames.len(), 4);
    }

    #[test]
    fn write_frames_helper() {
        let frames = vec![
            make_hello(),
            make_run("run-1"),
            make_event("run-1"),
            make_final("run-1"),
        ];

        let mut buf = Vec::new();
        let count = write_frames(&mut buf, &frames).unwrap();
        assert_eq!(count, 4);

        let read = read_all_frames(std::io::BufReader::new(buf.as_slice())).unwrap();
        assert_eq!(read.len(), 4);
    }

    #[test]
    fn state_machine_with_codec_roundtrip() {
        let mut state = ProtocolState::new();
        let frames = vec![
            make_hello(),
            make_run("run-1"),
            make_event("run-1"),
            make_event("run-1"),
            make_final("run-1"),
        ];

        for frame in &frames {
            let encoded = JsonlCodec::encode(frame).unwrap();
            let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
            state.advance(&decoded).unwrap();
        }

        assert_eq!(state.phase(), ProtocolPhase::Completed);
        assert_eq!(state.events_seen(), 2);
    }

    #[test]
    fn mock_stdin_fed_into_state_machine() {
        let frames = vec![
            make_hello(),
            make_run("run-1"),
            make_event("run-1"),
            make_final("run-1"),
        ];
        let mock = MockStdin::from_frames(&frames);
        let mut reader = FrameReader::new(mock);
        let mut state = ProtocolState::new();

        while let Some(frame) = reader.read_frame().unwrap() {
            state.advance(&frame).unwrap();
        }
        assert_eq!(state.phase(), ProtocolPhase::Completed);
    }

    #[test]
    fn validate_frame_hello_valid() {
        let frame = make_hello();
        let v = validate_frame(&frame, 16 * 1024 * 1024);
        assert!(v.valid, "issues: {:?}", v.issues);
    }

    #[test]
    fn validate_frame_hello_empty_contract_version() {
        let frame = Frame::Hello {
            contract_version: "".to_string(),
            backend: json!({"id": "test"}),
            capabilities: json!({}),
            mode: Value::Null,
        };
        let v = validate_frame(&frame, 16 * 1024 * 1024);
        assert!(!v.valid);
        assert!(v.issues.iter().any(|i| i.contains("contract_version")));
    }

    #[test]
    fn validate_frame_wrong_contract_prefix() {
        let frame = Frame::Hello {
            contract_version: "wrong/v1".to_string(),
            backend: json!({"id": "test"}),
            capabilities: json!({}),
            mode: Value::Null,
        };
        let v = validate_frame(&frame, 16 * 1024 * 1024);
        assert!(!v.valid);
        assert!(v.issues.iter().any(|i| i.contains("abp/v")));
    }

    #[test]
    fn validate_frame_size_exceeded() {
        let large_data = "x".repeat(1000);
        let frame = Frame::Run {
            id: "run-1".to_string(),
            work_order: json!({"data": large_data}),
        };
        let v = validate_frame(&frame, 100); // Very small limit
        assert!(!v.valid);
        assert!(v.issues.iter().any(|i| i.contains("exceeds limit")));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 7. FRAME DESERIALIZATION EDGE CASES (5+ tests)
// ═══════════════════════════════════════════════════════════════════════

mod deserialization_edge_cases {
    use super::*;

    #[test]
    fn deserialize_hello_from_raw_json() {
        let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"test"},"capabilities":{}}"#;
        let frame: Frame = serde_json::from_str(json).unwrap();
        assert!(matches!(frame, Frame::Hello { .. }));
    }

    #[test]
    fn deserialize_event_from_raw_json() {
        let json = r#"{"t":"event","ref_id":"run-1","event":{"ts":"2025-01-01T00:00:00Z","type":"assistant_delta","text":"hi"}}"#;
        let frame: Frame = serde_json::from_str(json).unwrap();
        assert!(matches!(frame, Frame::Event { .. }));
    }

    #[test]
    fn deserialize_fatal_with_null_ref_id() {
        let json = r#"{"t":"fatal","ref_id":null,"error":"boom"}"#;
        let frame: Frame = serde_json::from_str(json).unwrap();
        if let Frame::Fatal { ref_id, error } = &frame {
            assert!(ref_id.is_none());
            assert_eq!(error, "boom");
        } else {
            panic!("expected Fatal frame");
        }
    }

    #[test]
    fn unknown_tag_fails_deserialization() {
        let json = r#"{"t":"unknown_type","data":"test"}"#;
        let result = serde_json::from_str::<Frame>(json);
        assert!(result.is_err());
    }

    #[test]
    fn missing_tag_fails_deserialization() {
        let json = r#"{"contract_version":"abp/v0.1","backend":{}}"#;
        let result = serde_json::from_str::<Frame>(json);
        assert!(result.is_err());
    }

    #[test]
    fn extra_fields_are_tolerated() {
        let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"test"},"capabilities":{},"extra_field":"ignored","mode":null}"#;
        let frame: Frame = serde_json::from_str(json).unwrap();
        assert!(matches!(frame, Frame::Hello { .. }));
    }

    #[test]
    fn try_event_on_non_event_frame_fails() {
        let frame = make_hello();
        let result = frame.try_event::<Value>();
        assert!(result.is_err());
    }

    #[test]
    fn try_final_on_non_final_frame_fails() {
        let frame = make_event("run-1");
        let result = frame.try_final::<Value>();
        assert!(result.is_err());
    }

    #[test]
    fn try_event_extracts_typed_value() {
        let event_val = event_text_delta("hello");
        let frame = event_frame("run-1", event_val.clone());
        let (ref_id, extracted): (String, Value) = frame.try_event().unwrap();
        assert_eq!(ref_id, "run-1");
        assert_eq!(extracted["text"], "hello");
    }

    #[test]
    fn try_final_extracts_receipt() {
        let receipt = ReceiptBuilder::new("run-1", "test").build();
        let frame = final_frame("run-1", receipt.clone());
        let (ref_id, extracted): (String, Value) = frame.try_final().unwrap();
        assert_eq!(ref_id, "run-1");
        assert_eq!(extracted["outcome"], "complete");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 8. EXHAUSTIVE PHASE × FRAME MATRIX (complete coverage)
// ═══════════════════════════════════════════════════════════════════════

mod phase_frame_matrix {
    use super::*;

    /// For each non-terminal phase, attempt every frame type and verify
    /// the outcome matches the expected transition table.
    #[test]
    fn awaiting_hello_rejects_all_except_hello() {
        let non_hello_frames = vec![
            ("run", make_run("r")),
            ("event", make_event("r")),
            ("final", make_final("r")),
            ("fatal", make_fatal(Some("r"))),
            ("ping", make_ping(1)),
            ("pong", make_pong(1)),
            ("cancel", make_cancel("r")),
        ];

        for (name, frame) in non_hello_frames {
            let mut state = ProtocolState::new();
            let result = state.advance(&frame);
            assert!(result.is_err(), "AwaitingHello should reject {name} frame");
            assert_eq!(
                state.phase(),
                ProtocolPhase::Faulted,
                "should be Faulted after {name} in AwaitingHello"
            );
        }
    }

    #[test]
    fn awaiting_run_accepts_only_run_and_fatal() {
        let rejected = vec![
            ("hello", make_hello()),
            ("event", make_event("r")),
            ("final", make_final("r")),
            ("ping", make_ping(1)),
            ("pong", make_pong(1)),
            ("cancel", make_cancel("r")),
        ];

        for (name, frame) in rejected {
            let mut state = ProtocolState::new();
            state.advance(&make_hello()).unwrap();
            let result = state.advance(&frame);
            assert!(result.is_err(), "AwaitingRun should reject {name} frame");
            assert_eq!(
                state.phase(),
                ProtocolPhase::Faulted,
                "should be Faulted after {name} in AwaitingRun"
            );
        }
    }

    #[test]
    fn streaming_rejects_hello_and_run() {
        let rejected = vec![
            ("hello", make_hello()),
            ("run", make_run("r2")),
            ("cancel", make_cancel("run-1")),
        ];

        for (name, frame) in rejected {
            let mut state = ProtocolState::new();
            advance_to_streaming(&mut state, "run-1");
            let result = state.advance(&frame);
            assert!(result.is_err(), "Streaming should reject {name} frame");
            assert_eq!(
                state.phase(),
                ProtocolPhase::Faulted,
                "should be Faulted after {name} in Streaming"
            );
        }
    }

    #[test]
    fn completed_rejects_every_frame_type() {
        let all_frames = vec![
            ("hello", make_hello()),
            ("run", make_run("r")),
            ("event", make_event("run-1")),
            ("final", make_final("run-1")),
            ("fatal", make_fatal(Some("run-1"))),
            ("ping", make_ping(1)),
            ("pong", make_pong(1)),
            ("cancel", make_cancel("run-1")),
        ];

        for (name, frame) in all_frames {
            let mut state = ProtocolState::new();
            advance_to_completed(&mut state, "run-1");
            let result = state.advance(&frame);
            assert!(result.is_err(), "Completed should reject {name} frame");
            assert_eq!(
                state.phase(),
                ProtocolPhase::Faulted,
                "should be Faulted after {name} in Completed"
            );
        }
    }

    #[test]
    fn faulted_rejects_every_frame_type() {
        let all_frames = vec![
            make_hello(),
            make_run("r"),
            make_event("r"),
            make_final("r"),
            make_fatal(None),
            make_ping(1),
            make_pong(1),
            make_cancel("r"),
        ];

        let mut state = ProtocolState::new();
        // Fault the machine
        let _ = state.advance(&make_run("x"));
        assert_eq!(state.phase(), ProtocolPhase::Faulted);

        for frame in &all_frames {
            assert!(state.advance(frame).is_err());
            assert_eq!(state.phase(), ProtocolPhase::Faulted);
        }
    }
}
