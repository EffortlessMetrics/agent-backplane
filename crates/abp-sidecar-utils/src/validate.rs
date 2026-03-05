// SPDX-License-Identifier: MIT OR Apache-2.0
//! Validation helpers for [`Envelope`] sequences.
//!
//! These functions check structural invariants of the sidecar protocol:
//! - A valid `Hello` envelope has the correct contract version prefix.
//! - `ref_id` fields match the expected run identifier.
//! - A well-formed sequence starts with `Hello`, has zero or more `Event`s,
//!   and ends with exactly one `Final` or `Fatal`.

use abp_core::CONTRACT_VERSION;
use abp_protocol::{Envelope, ProtocolError, is_compatible_version};

/// Validate that an envelope is a well-formed `Hello`.
///
/// Checks:
/// - The envelope is the `Hello` variant.
/// - The `contract_version` is compatible with [`CONTRACT_VERSION`].
///
/// # Errors
///
/// Returns [`ProtocolError::Violation`] with a descriptive message.
pub fn validate_hello(envelope: &Envelope) -> Result<(), ProtocolError> {
    match envelope {
        Envelope::Hello {
            contract_version, ..
        } => {
            if !is_compatible_version(contract_version, CONTRACT_VERSION) {
                return Err(ProtocolError::Violation(format!(
                    "incompatible contract version: got \"{contract_version}\", \
                     expected compatible with \"{CONTRACT_VERSION}\""
                )));
            }
            Ok(())
        }
        _ => Err(ProtocolError::UnexpectedMessage {
            expected: "hello".into(),
            got: envelope_type_name(envelope).into(),
        }),
    }
}

/// Validate that an envelope's `ref_id` matches the expected value.
///
/// For `Event` and `Final` envelopes the `ref_id` field is checked directly.
/// For `Fatal` envelopes the optional `ref_id` is checked when present.
/// `Hello` and `Run` envelopes do not carry a `ref_id` and always pass.
///
/// # Errors
///
/// Returns [`ProtocolError::Violation`] on mismatch.
pub fn validate_ref_id(envelope: &Envelope, expected: &str) -> Result<(), ProtocolError> {
    let actual = match envelope {
        Envelope::Event { ref_id, .. } => Some(ref_id.as_str()),
        Envelope::Final { ref_id, .. } => Some(ref_id.as_str()),
        Envelope::Fatal {
            ref_id: Some(id), ..
        } => Some(id.as_str()),
        _ => None,
    };

    if let Some(actual) = actual {
        if actual != expected {
            return Err(ProtocolError::Violation(format!(
                "ref_id mismatch: expected \"{expected}\", got \"{actual}\""
            )));
        }
    }

    Ok(())
}

/// Validate that a sequence of envelopes follows the expected protocol order:
///
/// 1. Exactly one `Hello` (first position).
/// 2. Zero or more `Event` envelopes.
/// 3. Exactly one terminal (`Final` or `Fatal`) as the last envelope.
/// 4. No `Hello`, `Final`, or `Fatal` envelopes appear in the middle.
/// 5. No events appear after the terminal.
///
/// An empty slice is rejected.  A single `Hello` with no terminal is also
/// rejected (the stream was never completed).
///
/// # Errors
///
/// Returns [`ProtocolError::Violation`] describing the first violation found.
pub fn validate_sequence(envelopes: &[Envelope]) -> Result<(), ProtocolError> {
    if envelopes.is_empty() {
        return Err(ProtocolError::Violation("empty envelope sequence".into()));
    }

    // First envelope must be Hello.
    if !matches!(envelopes[0], Envelope::Hello { .. }) {
        return Err(ProtocolError::Violation(format!(
            "first envelope must be hello, got {}",
            envelope_type_name(&envelopes[0])
        )));
    }

    if envelopes.len() < 2 {
        return Err(ProtocolError::Violation(
            "sequence must contain at least a hello and a terminal envelope".into(),
        ));
    }

    // Last envelope must be Final or Fatal.
    let last = &envelopes[envelopes.len() - 1];
    if !matches!(last, Envelope::Final { .. } | Envelope::Fatal { .. }) {
        return Err(ProtocolError::Violation(format!(
            "last envelope must be final or fatal, got {}",
            envelope_type_name(last)
        )));
    }

    // Middle envelopes (index 1..len-1) must all be Event.
    for (i, env) in envelopes.iter().enumerate().skip(1) {
        if i == envelopes.len() - 1 {
            // Already validated the last envelope above.
            break;
        }
        match env {
            Envelope::Event { .. } => {}
            Envelope::Run { .. } => {}
            _ => {
                return Err(ProtocolError::Violation(format!(
                    "unexpected {} at position {i}; only event envelopes are allowed between hello and terminal",
                    envelope_type_name(env)
                )));
            }
        }
    }

    // Check for duplicate terminals.
    let terminal_count = envelopes
        .iter()
        .filter(|e| matches!(e, Envelope::Final { .. } | Envelope::Fatal { .. }))
        .count();
    if terminal_count > 1 {
        return Err(ProtocolError::Violation(format!(
            "multiple terminal envelopes found ({terminal_count}); expected exactly one"
        )));
    }

    // Check for duplicate hellos.
    let hello_count = envelopes
        .iter()
        .filter(|e| matches!(e, Envelope::Hello { .. }))
        .count();
    if hello_count > 1 {
        return Err(ProtocolError::Violation(format!(
            "multiple hello envelopes found ({hello_count}); expected exactly one"
        )));
    }

    Ok(())
}

/// Return a human-readable discriminant name for an [`Envelope`].
fn envelope_type_name(envelope: &Envelope) -> &'static str {
    match envelope {
        Envelope::Hello { .. } => "hello",
        Envelope::Run { .. } => "run",
        Envelope::Event { .. } => "event",
        Envelope::Final { .. } => "final",
        Envelope::Fatal { .. } => "fatal",
    }
}
