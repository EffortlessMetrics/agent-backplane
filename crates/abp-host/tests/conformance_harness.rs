// SPDX-License-Identifier: MIT OR Apache-2.0
//! Reusable conformance test harness for validating sidecar protocol
//! implementations against the protocol spec.
//!
//! Each validation function accepts parsed [`Envelope`] values and returns a
//! vector of [`ConformanceResult`] checks that can be inspected individually.

use std::collections::HashSet;
use std::io::BufReader;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, CapabilityManifest,
    ExecutionMode, ReceiptBuilder, receipt_hash,
};
use abp_protocol::{Envelope, JsonlCodec, parse_version};
use chrono::{DateTime, Utc};

// =========================================================================
// ConformanceResult
// =========================================================================

/// Records the outcome of a single conformance check.
#[derive(Debug, Clone)]
struct ConformanceResult {
    name: String,
    passed: bool,
    detail: Option<String>,
}

impl ConformanceResult {
    fn pass(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            passed: true,
            detail: None,
        }
    }
    fn fail(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            passed: false,
            detail: Some(detail.into()),
        }
    }
}

// =========================================================================
// Validation functions
// =========================================================================

/// Validate a `Hello` envelope against the protocol spec.
///
/// Checks: `hello_has_backend`, `hello_has_contract_version`,
/// `hello_version_format_valid`, `hello_has_capabilities`, `hello_has_mode`.
fn validate_hello(hello: &Envelope) -> Vec<ConformanceResult> {
    let mut r = Vec::new();
    match hello {
        Envelope::Hello {
            contract_version,
            backend,
            capabilities: _,
            mode: _,
        } => {
            if backend.id.is_empty() {
                r.push(ConformanceResult::fail(
                    "hello_has_backend",
                    "backend.id is empty",
                ));
            } else {
                r.push(ConformanceResult::pass("hello_has_backend"));
            }

            if contract_version.is_empty() {
                r.push(ConformanceResult::fail(
                    "hello_has_contract_version",
                    "contract_version is empty",
                ));
            } else {
                r.push(ConformanceResult::pass("hello_has_contract_version"));
            }

            if parse_version(contract_version).is_some() {
                r.push(ConformanceResult::pass("hello_version_format_valid"));
            } else {
                r.push(ConformanceResult::fail(
                    "hello_version_format_valid",
                    format!("cannot parse version: {contract_version}"),
                ));
            }

            // capabilities and mode are always present in the Hello variant
            r.push(ConformanceResult::pass("hello_has_capabilities"));
            r.push(ConformanceResult::pass("hello_has_mode"));
        }
        other => {
            r.push(ConformanceResult::fail(
                "hello_has_backend",
                format!("expected Hello, got {}", envelope_tag(other)),
            ));
        }
    }
    r
}

/// Validate a stream of `Event` envelopes.
///
/// Checks: `events_all_event_variant`, `events_ref_id_consistent`,
/// `events_well_ordered`, `events_no_duplicate_timestamps`,
/// `events_valid_agent_event_kind`.
fn validate_event_stream(events: &[Envelope]) -> Vec<ConformanceResult> {
    let mut r = Vec::new();
    let mut ref_ids: Vec<&str> = Vec::new();
    let mut agent_events: Vec<&AgentEvent> = Vec::new();
    let mut all_event = true;

    for (i, env) in events.iter().enumerate() {
        match env {
            Envelope::Event { ref_id, event } => {
                ref_ids.push(ref_id.as_str());
                agent_events.push(event);
            }
            _ => {
                all_event = false;
                r.push(ConformanceResult::fail(
                    "events_all_event_variant",
                    format!("index {i} is {}, not event", envelope_tag(env)),
                ));
            }
        }
    }
    if all_event {
        r.push(ConformanceResult::pass("events_all_event_variant"));
    }

    // ref_id consistency
    if let Some(first) = ref_ids.first() {
        if ref_ids.iter().all(|id| id == first) {
            r.push(ConformanceResult::pass("events_ref_id_consistent"));
        } else {
            r.push(ConformanceResult::fail(
                "events_ref_id_consistent",
                "not all ref_ids match",
            ));
        }
    } else {
        r.push(ConformanceResult::pass("events_ref_id_consistent"));
    }

    // ordering: RunStarted must precede RunCompleted
    let mut saw_started = false;
    let mut bad_order = false;
    for ev in &agent_events {
        match &ev.kind {
            AgentEventKind::RunStarted { .. } => saw_started = true,
            AgentEventKind::RunCompleted { .. } if !saw_started => bad_order = true,
            _ => {}
        }
    }
    if bad_order {
        r.push(ConformanceResult::fail(
            "events_well_ordered",
            "RunCompleted appeared before RunStarted",
        ));
    } else {
        r.push(ConformanceResult::pass("events_well_ordered"));
    }

    // no duplicate timestamps
    let millis: Vec<i64> = agent_events
        .iter()
        .map(|e| e.ts.timestamp_millis())
        .collect();
    let unique = millis.iter().collect::<HashSet<_>>().len();
    if unique == millis.len() {
        r.push(ConformanceResult::pass("events_no_duplicate_timestamps"));
    } else {
        r.push(ConformanceResult::fail(
            "events_no_duplicate_timestamps",
            format!(
                "{} duplicate(s) among {} events",
                millis.len() - unique,
                millis.len()
            ),
        ));
    }

    // all events have valid AgentEventKind (deserialized ⇒ valid)
    r.push(ConformanceResult::pass("events_valid_agent_event_kind"));

    r
}

/// Validate a `Final` envelope.
///
/// Checks: `final_has_ref_id`, `final_receipt_hash_valid`,
/// `final_receipt_has_contract_version`.
fn validate_final(final_env: &Envelope) -> Vec<ConformanceResult> {
    let mut r = Vec::new();
    match final_env {
        Envelope::Final { ref_id, receipt } => {
            if ref_id.is_empty() {
                r.push(ConformanceResult::fail(
                    "final_has_ref_id",
                    "ref_id is empty",
                ));
            } else {
                r.push(ConformanceResult::pass("final_has_ref_id"));
            }

            match &receipt.receipt_sha256 {
                Some(hash) if !hash.is_empty() => match receipt_hash(receipt) {
                    Ok(computed) if computed == *hash => {
                        r.push(ConformanceResult::pass("final_receipt_hash_valid"));
                    }
                    Ok(computed) => {
                        r.push(ConformanceResult::fail(
                            "final_receipt_hash_valid",
                            format!("hash mismatch: got {hash}, expected {computed}"),
                        ));
                    }
                    Err(e) => {
                        r.push(ConformanceResult::fail(
                            "final_receipt_hash_valid",
                            format!("cannot compute hash: {e}"),
                        ));
                    }
                },
                _ => {
                    // No hash — protocol permits this
                    r.push(ConformanceResult::pass("final_receipt_hash_valid"));
                }
            }

            if receipt.meta.contract_version.is_empty() {
                r.push(ConformanceResult::fail(
                    "final_receipt_has_contract_version",
                    "receipt.meta.contract_version is empty",
                ));
            } else {
                r.push(ConformanceResult::pass(
                    "final_receipt_has_contract_version",
                ));
            }
        }
        other => {
            r.push(ConformanceResult::fail(
                "final_is_final_variant",
                format!("expected Final, got {}", envelope_tag(other)),
            ));
        }
    }
    r
}

/// Validate a full protocol sequence (hello → events → final/fatal).
///
/// Checks: `sequence_starts_with_hello`, `sequence_ends_with_final_or_fatal`,
/// `sequence_middle_valid`, `sequence_ref_id_correlation`.
fn validate_protocol_sequence(envelopes: &[Envelope]) -> Vec<ConformanceResult> {
    let mut r = Vec::new();

    if envelopes.is_empty() {
        r.push(ConformanceResult::fail(
            "sequence_non_empty",
            "empty sequence",
        ));
        return r;
    }

    // First must be Hello
    if matches!(envelopes[0], Envelope::Hello { .. }) {
        r.push(ConformanceResult::pass("sequence_starts_with_hello"));
    } else {
        r.push(ConformanceResult::fail(
            "sequence_starts_with_hello",
            format!("first is {}, not hello", envelope_tag(&envelopes[0])),
        ));
    }

    // Last must be Final or Fatal
    let last = envelopes.last().unwrap();
    if matches!(last, Envelope::Final { .. } | Envelope::Fatal { .. }) {
        r.push(ConformanceResult::pass("sequence_ends_with_final_or_fatal"));
    } else {
        r.push(ConformanceResult::fail(
            "sequence_ends_with_final_or_fatal",
            format!("last is {}, not final/fatal", envelope_tag(last)),
        ));
    }

    // Middle envelopes (between first and last) must all be Event
    let mid_start = 1;
    let mid_end = envelopes.len().saturating_sub(1);
    if mid_start < mid_end {
        let mut ok = true;
        for (i, env) in envelopes[mid_start..mid_end].iter().enumerate() {
            if !matches!(env, Envelope::Event { .. }) {
                ok = false;
                r.push(ConformanceResult::fail(
                    "sequence_middle_valid",
                    format!(
                        "index {} is {}, expected event",
                        i + mid_start,
                        envelope_tag(env)
                    ),
                ));
                break;
            }
        }
        if ok {
            r.push(ConformanceResult::pass("sequence_middle_valid"));
        }
    } else {
        r.push(ConformanceResult::pass("sequence_middle_valid"));
    }

    // ref_id correlation across events / final / fatal
    let mut ids: Vec<&str> = Vec::new();
    for env in envelopes {
        match env {
            Envelope::Event { ref_id, .. } | Envelope::Final { ref_id, .. } => {
                ids.push(ref_id.as_str());
            }
            Envelope::Fatal {
                ref_id: Some(id),
                error_code: _,
                ..
            } => ids.push(id.as_str()),
            _ => {}
        }
    }
    if let Some(first) = ids.first() {
        if ids.iter().all(|id| id == first) {
            r.push(ConformanceResult::pass("sequence_ref_id_correlation"));
        } else {
            r.push(ConformanceResult::fail(
                "sequence_ref_id_correlation",
                "not all ref_ids match across the sequence",
            ));
        }
    } else {
        r.push(ConformanceResult::pass("sequence_ref_id_correlation"));
    }

    r
}

// =========================================================================
// Helpers
// =========================================================================

fn envelope_tag(env: &Envelope) -> &'static str {
    match env {
        Envelope::Hello { .. } => "hello",
        Envelope::Run { .. } => "run",
        Envelope::Event { .. } => "event",
        Envelope::Final { .. } => "final",
        Envelope::Fatal { .. } => "fatal",
    }
}

/// Fixed base timestamp to keep tests deterministic.
fn ts_at(offset_ms: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(1_700_000_000_000 + offset_ms).unwrap()
}

fn make_hello() -> Envelope {
    Envelope::hello(
        BackendIdentity {
            id: "test-sidecar".into(),
            backend_version: Some("1.0".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    )
}

fn make_event(ref_id: &str, kind: AgentEventKind, offset_ms: i64) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: AgentEvent {
            ts: ts_at(offset_ms),
            kind,
            ext: None,
        },
    }
}

fn make_final(ref_id: &str) -> Envelope {
    let receipt = ReceiptBuilder::new("test-sidecar").build();
    Envelope::Final {
        ref_id: ref_id.into(),
        receipt,
    }
}

fn assert_all_pass(results: &[ConformanceResult]) {
    let fails: Vec<_> = results.iter().filter(|c| !c.passed).collect();
    if !fails.is_empty() {
        let msgs: Vec<_> = fails
            .iter()
            .map(|c| format!("  FAIL {}: {}", c.name, c.detail.as_deref().unwrap_or("?")))
            .collect();
        panic!("{} check(s) failed:\n{}", fails.len(), msgs.join("\n"));
    }
}

fn assert_has_failure(results: &[ConformanceResult], name: &str) {
    assert!(
        results.iter().any(|c| !c.passed && c.name == name),
        "expected failure for '{name}', but got: {results:?}"
    );
}

// =========================================================================
// Tests — validate_hello (5 tests)
// =========================================================================

#[test]
fn test_validate_hello_valid() {
    let results = validate_hello(&make_hello());
    assert_all_pass(&results);
    assert_eq!(results.len(), 5);
}

#[test]
fn test_validate_hello_empty_backend() {
    let hello = Envelope::Hello {
        contract_version: CONTRACT_VERSION.into(),
        backend: BackendIdentity {
            id: String::new(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::Mapped,
    };
    assert_has_failure(&validate_hello(&hello), "hello_has_backend");
}

#[test]
fn test_validate_hello_bad_version() {
    let hello = Envelope::Hello {
        contract_version: "not-a-version".into(),
        backend: BackendIdentity {
            id: "x".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::Mapped,
    };
    assert_has_failure(&validate_hello(&hello), "hello_version_format_valid");
}

#[test]
fn test_validate_hello_non_hello_envelope() {
    let fatal = Envelope::Fatal {
        ref_id: None,
        error: "boom".into(),
        error_code: None,
    };
    assert_has_failure(&validate_hello(&fatal), "hello_has_backend");
}

#[test]
fn test_validate_hello_passthrough_mode() {
    let hello = Envelope::hello_with_mode(
        BackendIdentity {
            id: "pt".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
        ExecutionMode::Passthrough,
    );
    assert_all_pass(&validate_hello(&hello));
}

// =========================================================================
// Tests — validate_event_stream (5 tests)
// =========================================================================

#[test]
fn test_validate_events_valid() {
    let id = "run-1";
    let events = vec![
        make_event(
            id,
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            0,
        ),
        make_event(id, AgentEventKind::AssistantDelta { text: "hi".into() }, 1),
        make_event(
            id,
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            2,
        ),
    ];
    assert_all_pass(&validate_event_stream(&events));
}

#[test]
fn test_validate_events_mismatched_refs() {
    let events = vec![
        make_event(
            "run-1",
            AgentEventKind::RunStarted {
                message: "a".into(),
            },
            0,
        ),
        make_event(
            "run-2",
            AgentEventKind::RunCompleted {
                message: "b".into(),
            },
            1,
        ),
    ];
    assert_has_failure(&validate_event_stream(&events), "events_ref_id_consistent");
}

#[test]
fn test_validate_events_bad_order() {
    let id = "run-1";
    let events = vec![
        make_event(
            id,
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            0,
        ),
        make_event(
            id,
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            1,
        ),
    ];
    assert_has_failure(&validate_event_stream(&events), "events_well_ordered");
}

#[test]
fn test_validate_events_duplicate_timestamps() {
    let ts = ts_at(0);
    let events = vec![
        Envelope::Event {
            ref_id: "r".into(),
            event: AgentEvent {
                ts,
                kind: AgentEventKind::RunStarted {
                    message: "a".into(),
                },
                ext: None,
            },
        },
        Envelope::Event {
            ref_id: "r".into(),
            event: AgentEvent {
                ts,
                kind: AgentEventKind::RunCompleted {
                    message: "b".into(),
                },
                ext: None,
            },
        },
    ];
    assert_has_failure(
        &validate_event_stream(&events),
        "events_no_duplicate_timestamps",
    );
}

#[test]
fn test_validate_events_empty_stream() {
    assert_all_pass(&validate_event_stream(&[]));
}

// =========================================================================
// Tests — validate_final (5 tests)
// =========================================================================

#[test]
fn test_validate_final_valid() {
    assert_all_pass(&validate_final(&make_final("run-1")));
}

#[test]
fn test_validate_final_empty_ref_id() {
    let receipt = ReceiptBuilder::new("test").build();
    let f = Envelope::Final {
        ref_id: String::new(),
        receipt,
    };
    assert_has_failure(&validate_final(&f), "final_has_ref_id");
}

#[test]
fn test_validate_final_valid_hash() {
    let receipt = ReceiptBuilder::new("test").build().with_hash().unwrap();
    let f = Envelope::Final {
        ref_id: "run-1".into(),
        receipt,
    };
    assert_all_pass(&validate_final(&f));
}

#[test]
fn test_validate_final_bad_hash() {
    let mut receipt = ReceiptBuilder::new("test").build();
    receipt.receipt_sha256 = Some("badhash".into());
    let f = Envelope::Final {
        ref_id: "run-1".into(),
        receipt,
    };
    assert_has_failure(&validate_final(&f), "final_receipt_hash_valid");
}

#[test]
fn test_validate_final_empty_contract_version() {
    let mut receipt = ReceiptBuilder::new("test").build();
    receipt.meta.contract_version = String::new();
    let f = Envelope::Final {
        ref_id: "run-1".into(),
        receipt,
    };
    assert_has_failure(&validate_final(&f), "final_receipt_has_contract_version");
}

// =========================================================================
// Tests — validate_protocol_sequence (6 tests)
// =========================================================================

#[test]
fn test_validate_sequence_valid() {
    let id = "run-1";
    let seq = vec![
        make_hello(),
        make_event(
            id,
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            0,
        ),
        make_event(
            id,
            AgentEventKind::AssistantMessage { text: "hi".into() },
            1,
        ),
        make_event(
            id,
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            2,
        ),
        make_final(id),
    ];
    assert_all_pass(&validate_protocol_sequence(&seq));
}

#[test]
fn test_validate_sequence_no_hello() {
    let seq = vec![
        make_event(
            "r",
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            0,
        ),
        make_final("r"),
    ];
    assert_has_failure(
        &validate_protocol_sequence(&seq),
        "sequence_starts_with_hello",
    );
}

#[test]
fn test_validate_sequence_no_terminal() {
    let seq = vec![
        make_hello(),
        make_event(
            "r",
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            0,
        ),
    ];
    assert_has_failure(
        &validate_protocol_sequence(&seq),
        "sequence_ends_with_final_or_fatal",
    );
}

#[test]
fn test_validate_sequence_fatal_ending() {
    let seq = vec![
        make_hello(),
        make_event(
            "r",
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            0,
        ),
        Envelope::Fatal {
            ref_id: Some("r".into()),
            error: "crash".into(),
            error_code: None,
        },
    ];
    assert_all_pass(&validate_protocol_sequence(&seq));
}

#[test]
fn test_validate_sequence_ref_id_mismatch() {
    let seq = vec![
        make_hello(),
        make_event(
            "run-1",
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            0,
        ),
        make_event(
            "run-2",
            AgentEventKind::AssistantDelta { text: "x".into() },
            1,
        ),
        make_final("run-1"),
    ];
    assert_has_failure(
        &validate_protocol_sequence(&seq),
        "sequence_ref_id_correlation",
    );
}

#[test]
fn test_validate_jsonl_round_trip() {
    let id = "run-abc";
    let seq = vec![
        make_hello(),
        make_event(
            id,
            AgentEventKind::RunStarted {
                message: "go".into(),
            },
            0,
        ),
        make_event(id, AgentEventKind::AssistantDelta { text: "tok".into() }, 1),
        make_final(id),
    ];

    // Encode to JSONL bytes
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &seq).unwrap();

    // Decode back
    let reader = BufReader::new(buf.as_slice());
    let decoded: Vec<Envelope> = JsonlCodec::decode_stream(reader)
        .collect::<Result<_, _>>()
        .unwrap();

    assert_eq!(decoded.len(), seq.len());
    assert_all_pass(&validate_protocol_sequence(&decoded));
}
