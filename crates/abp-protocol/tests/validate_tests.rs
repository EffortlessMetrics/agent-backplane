// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for the `validate` module.

use std::collections::BTreeMap;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, Capability, CapabilityManifest, Outcome,
    ReceiptBuilder, SupportLevel, WorkOrderBuilder,
};
use abp_protocol::validate::{
    EnvelopeValidator, SequenceError, ValidationError, ValidationWarning,
};
use abp_protocol::Envelope;
use chrono::Utc;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_backend() -> BackendIdentity {
    BackendIdentity {
        id: "test-backend".into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: None,
    }
}

fn test_capabilities() -> CapabilityManifest {
    let mut caps = BTreeMap::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps
}

fn test_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn hello() -> Envelope {
    Envelope::hello(test_backend(), test_capabilities())
}

fn run_env(id: &str) -> Envelope {
    Envelope::Run {
        id: id.into(),
        work_order: WorkOrderBuilder::new("do something").build(),
    }
}

fn event_env(ref_id: &str) -> Envelope {
    Envelope::Event {
        ref_id: ref_id.into(),
        event: test_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
    }
}

fn final_env(ref_id: &str) -> Envelope {
    Envelope::Final {
        ref_id: ref_id.into(),
        receipt: ReceiptBuilder::new("test-backend")
            .outcome(Outcome::Complete)
            .build(),
    }
}

fn fatal_env(ref_id: Option<&str>, error: &str) -> Envelope {
    Envelope::Fatal {
        ref_id: ref_id.map(Into::into),
        error: error.into(),
    }
}

// ---------------------------------------------------------------------------
// Single-envelope validation: Hello
// ---------------------------------------------------------------------------

#[test]
fn valid_hello_has_no_errors() {
    let v = EnvelopeValidator::new();
    let r = v.validate(&hello());
    assert!(r.valid);
    assert!(r.errors.is_empty());
}

#[test]
fn hello_empty_contract_version() {
    let v = EnvelopeValidator::new();
    let env = Envelope::Hello {
        contract_version: String::new(),
        backend: test_backend(),
        capabilities: test_capabilities(),
        mode: Default::default(),
    };
    let r = v.validate(&env);
    assert!(!r.valid);
    assert!(r.errors.contains(&ValidationError::EmptyField {
        field: "contract_version".into(),
    }));
}

#[test]
fn hello_invalid_contract_version() {
    let v = EnvelopeValidator::new();
    let env = Envelope::Hello {
        contract_version: "not-a-version".into(),
        backend: test_backend(),
        capabilities: test_capabilities(),
        mode: Default::default(),
    };
    let r = v.validate(&env);
    assert!(!r.valid);
    assert!(r.errors.contains(&ValidationError::InvalidVersion {
        version: "not-a-version".into(),
    }));
}

#[test]
fn hello_empty_backend_id() {
    let v = EnvelopeValidator::new();
    let env = Envelope::Hello {
        contract_version: "abp/v0.1".into(),
        backend: BackendIdentity {
            id: String::new(),
            backend_version: Some("1.0".into()),
            adapter_version: None,
        },
        capabilities: test_capabilities(),
        mode: Default::default(),
    };
    let r = v.validate(&env);
    assert!(!r.valid);
    assert!(r.errors.contains(&ValidationError::EmptyField {
        field: "backend.id".into(),
    }));
}

#[test]
fn hello_warns_on_missing_optional_backend_fields() {
    let v = EnvelopeValidator::new();
    let env = Envelope::Hello {
        contract_version: "abp/v0.1".into(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: test_capabilities(),
        mode: Default::default(),
    };
    let r = v.validate(&env);
    assert!(r.valid);
    assert!(r.warnings.contains(&ValidationWarning::MissingOptionalField {
        field: "backend.backend_version".into(),
    }));
    assert!(r.warnings.contains(&ValidationWarning::MissingOptionalField {
        field: "backend.adapter_version".into(),
    }));
}

// ---------------------------------------------------------------------------
// Single-envelope validation: Run
// ---------------------------------------------------------------------------

#[test]
fn valid_run_has_no_errors() {
    let v = EnvelopeValidator::new();
    let r = v.validate(&run_env("run-1"));
    assert!(r.valid);
    assert!(r.errors.is_empty());
}

#[test]
fn run_empty_id() {
    let v = EnvelopeValidator::new();
    let env = Envelope::Run {
        id: String::new(),
        work_order: WorkOrderBuilder::new("task").build(),
    };
    let r = v.validate(&env);
    assert!(!r.valid);
    assert!(r.errors.contains(&ValidationError::EmptyField {
        field: "id".into(),
    }));
}

#[test]
fn run_empty_task() {
    let v = EnvelopeValidator::new();
    let env = Envelope::Run {
        id: "run-1".into(),
        work_order: WorkOrderBuilder::new("").build(),
    };
    let r = v.validate(&env);
    assert!(!r.valid);
    assert!(r.errors.contains(&ValidationError::EmptyField {
        field: "work_order.task".into(),
    }));
}

// ---------------------------------------------------------------------------
// Single-envelope validation: Event
// ---------------------------------------------------------------------------

#[test]
fn valid_event_has_no_errors() {
    let v = EnvelopeValidator::new();
    let r = v.validate(&event_env("run-1"));
    assert!(r.valid);
}

#[test]
fn event_empty_ref_id() {
    let v = EnvelopeValidator::new();
    let env = Envelope::Event {
        ref_id: String::new(),
        event: test_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
    };
    let r = v.validate(&env);
    assert!(!r.valid);
    assert!(r.errors.contains(&ValidationError::EmptyField {
        field: "ref_id".into(),
    }));
}

// ---------------------------------------------------------------------------
// Single-envelope validation: Final
// ---------------------------------------------------------------------------

#[test]
fn valid_final_has_no_errors() {
    let v = EnvelopeValidator::new();
    let r = v.validate(&final_env("run-1"));
    assert!(r.valid);
}

#[test]
fn final_empty_ref_id() {
    let v = EnvelopeValidator::new();
    let env = Envelope::Final {
        ref_id: String::new(),
        receipt: ReceiptBuilder::new("test").outcome(Outcome::Complete).build(),
    };
    let r = v.validate(&env);
    assert!(!r.valid);
    assert!(r.errors.contains(&ValidationError::EmptyField {
        field: "ref_id".into(),
    }));
}

// ---------------------------------------------------------------------------
// Single-envelope validation: Fatal
// ---------------------------------------------------------------------------

#[test]
fn valid_fatal_has_no_errors() {
    let v = EnvelopeValidator::new();
    let r = v.validate(&fatal_env(Some("run-1"), "boom"));
    assert!(r.valid);
}

#[test]
fn fatal_empty_error() {
    let v = EnvelopeValidator::new();
    let env = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: String::new(),
    };
    let r = v.validate(&env);
    assert!(!r.valid);
    assert!(r.errors.contains(&ValidationError::EmptyField {
        field: "error".into(),
    }));
}

#[test]
fn fatal_warns_on_missing_ref_id() {
    let v = EnvelopeValidator::new();
    let r = v.validate(&fatal_env(None, "crash"));
    assert!(r.valid);
    assert!(r.warnings.contains(&ValidationWarning::MissingOptionalField {
        field: "ref_id".into(),
    }));
}

// ---------------------------------------------------------------------------
// Display impls
// ---------------------------------------------------------------------------

#[test]
fn validation_error_display() {
    let e = ValidationError::MissingField {
        field: "foo".into(),
    };
    assert_eq!(e.to_string(), "missing required field: foo");

    let e = ValidationError::InvalidVersion {
        version: "bad".into(),
    };
    assert!(e.to_string().contains("bad"));

    let e = ValidationError::EmptyField {
        field: "bar".into(),
    };
    assert!(e.to_string().contains("bar"));

    let e = ValidationError::InvalidValue {
        field: "f".into(),
        value: "v".into(),
        expected: "e".into(),
    };
    assert!(e.to_string().contains("f"));
}

#[test]
fn validation_warning_display() {
    let w = ValidationWarning::DeprecatedField {
        field: "old".into(),
    };
    assert!(w.to_string().contains("old"));

    let w = ValidationWarning::LargePayload {
        size: 100,
        max_recommended: 50,
    };
    assert!(w.to_string().contains("100"));

    let w = ValidationWarning::MissingOptionalField {
        field: "opt".into(),
    };
    assert!(w.to_string().contains("opt"));
}

// ---------------------------------------------------------------------------
// Sequence validation: happy path
// ---------------------------------------------------------------------------

#[test]
fn valid_sequence_returns_no_errors() {
    let v = EnvelopeValidator::new();
    let seq = vec![
        hello(),
        run_env("run-1"),
        event_env("run-1"),
        event_env("run-1"),
        final_env("run-1"),
    ];
    let errors = v.validate_sequence(&seq);
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

#[test]
fn valid_sequence_with_fatal() {
    let v = EnvelopeValidator::new();
    let seq = vec![
        hello(),
        run_env("run-1"),
        fatal_env(Some("run-1"), "oom"),
    ];
    let errors = v.validate_sequence(&seq);
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

// ---------------------------------------------------------------------------
// Sequence validation: error cases
// ---------------------------------------------------------------------------

#[test]
fn empty_sequence_reports_missing_hello_and_terminal() {
    let v = EnvelopeValidator::new();
    let errors = v.validate_sequence(&[]);
    assert!(errors.contains(&SequenceError::MissingHello));
    assert!(errors.contains(&SequenceError::MissingTerminal));
}

#[test]
fn missing_hello() {
    let v = EnvelopeValidator::new();
    let seq = vec![run_env("run-1"), event_env("run-1"), final_env("run-1")];
    let errors = v.validate_sequence(&seq);
    assert!(errors.contains(&SequenceError::MissingHello));
}

#[test]
fn hello_not_first() {
    let v = EnvelopeValidator::new();
    let seq = vec![
        run_env("run-1"),
        hello(),
        event_env("run-1"),
        final_env("run-1"),
    ];
    let errors = v.validate_sequence(&seq);
    assert!(errors.contains(&SequenceError::HelloNotFirst { position: 1 }));
}

#[test]
fn missing_terminal() {
    let v = EnvelopeValidator::new();
    let seq = vec![hello(), run_env("run-1"), event_env("run-1")];
    let errors = v.validate_sequence(&seq);
    assert!(errors.contains(&SequenceError::MissingTerminal));
}

#[test]
fn multiple_terminals() {
    let v = EnvelopeValidator::new();
    let seq = vec![
        hello(),
        run_env("run-1"),
        final_env("run-1"),
        fatal_env(Some("run-1"), "oops"),
    ];
    let errors = v.validate_sequence(&seq);
    assert!(errors.contains(&SequenceError::MultipleTerminals));
}

#[test]
fn ref_id_mismatch_on_event() {
    let v = EnvelopeValidator::new();
    let seq = vec![
        hello(),
        run_env("run-1"),
        event_env("wrong-id"),
        final_env("run-1"),
    ];
    let errors = v.validate_sequence(&seq);
    assert!(errors.contains(&SequenceError::RefIdMismatch {
        expected: "run-1".into(),
        found: "wrong-id".into(),
    }));
}

#[test]
fn ref_id_mismatch_on_final() {
    let v = EnvelopeValidator::new();
    let seq = vec![
        hello(),
        run_env("run-1"),
        event_env("run-1"),
        final_env("wrong-id"),
    ];
    let errors = v.validate_sequence(&seq);
    assert!(errors.contains(&SequenceError::RefIdMismatch {
        expected: "run-1".into(),
        found: "wrong-id".into(),
    }));
}

#[test]
fn ref_id_mismatch_on_fatal() {
    let v = EnvelopeValidator::new();
    let seq = vec![
        hello(),
        run_env("run-1"),
        fatal_env(Some("wrong-id"), "err"),
    ];
    let errors = v.validate_sequence(&seq);
    assert!(errors.contains(&SequenceError::RefIdMismatch {
        expected: "run-1".into(),
        found: "wrong-id".into(),
    }));
}

#[test]
fn event_before_run_is_out_of_order() {
    let v = EnvelopeValidator::new();
    let seq = vec![
        hello(),
        event_env("run-1"),
        run_env("run-1"),
        final_env("run-1"),
    ];
    let errors = v.validate_sequence(&seq);
    assert!(errors.contains(&SequenceError::OutOfOrderEvents));
}

#[test]
fn sequence_error_display() {
    let e = SequenceError::MissingHello;
    assert!(e.to_string().contains("Hello"));

    let e = SequenceError::MissingTerminal;
    assert!(e.to_string().contains("terminal"));

    let e = SequenceError::HelloNotFirst { position: 3 };
    assert!(e.to_string().contains('3'));

    let e = SequenceError::MultipleTerminals;
    assert!(e.to_string().contains("multiple"));

    let e = SequenceError::RefIdMismatch {
        expected: "a".into(),
        found: "b".into(),
    };
    assert!(e.to_string().contains('a'));
    assert!(e.to_string().contains('b'));

    let e = SequenceError::OutOfOrderEvents;
    assert!(!e.to_string().is_empty());
}

#[test]
fn validator_default_trait() {
    // EnvelopeValidator implements Default via new().
    let v = EnvelopeValidator::default();
    let r = v.validate(&hello());
    assert!(r.valid);
}
