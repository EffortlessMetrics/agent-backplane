// SPDX-License-Identifier: MIT OR Apache-2.0
//! Protocol envelope validation.

use abp_core::CONTRACT_VERSION;
use abp_protocol::Envelope;

use crate::{ValidationErrorKind, ValidationErrors, Validator};

/// Validates a protocol [`Envelope`].
#[derive(Debug, Default)]
pub struct EnvelopeValidator;

impl Validator<Envelope> for EnvelopeValidator {
    fn validate(&self, envelope: &Envelope) -> Result<(), ValidationErrors> {
        let mut errs = ValidationErrors::new();

        match envelope {
            Envelope::Hello {
                contract_version,
                backend,
                ..
            } => {
                if contract_version.trim().is_empty() {
                    errs.add(
                        "contract_version",
                        ValidationErrorKind::Required,
                        "hello envelope must include contract_version",
                    );
                }
                if !contract_version.starts_with("abp/v") {
                    errs.add(
                        "contract_version",
                        ValidationErrorKind::InvalidFormat,
                        "contract_version must start with 'abp/v'",
                    );
                }
                if backend.id.trim().is_empty() {
                    errs.add(
                        "backend.id",
                        ValidationErrorKind::Required,
                        "backend id must not be empty in hello",
                    );
                }
            }
            Envelope::Run { id, work_order } => {
                if id.trim().is_empty() {
                    errs.add(
                        "id",
                        ValidationErrorKind::Required,
                        "run envelope must have a non-empty id",
                    );
                }
                if work_order.task.trim().is_empty() {
                    errs.add(
                        "work_order.task",
                        ValidationErrorKind::Required,
                        "run envelope work_order.task must not be empty",
                    );
                }
            }
            Envelope::Event { ref_id, .. } => {
                if ref_id.trim().is_empty() {
                    errs.add(
                        "ref_id",
                        ValidationErrorKind::Required,
                        "event envelope must have a non-empty ref_id",
                    );
                }
            }
            Envelope::Final { ref_id, .. } => {
                if ref_id.trim().is_empty() {
                    errs.add(
                        "ref_id",
                        ValidationErrorKind::Required,
                        "final envelope must have a non-empty ref_id",
                    );
                }
            }
            Envelope::Fatal { error, .. } => {
                if error.trim().is_empty() {
                    errs.add(
                        "error",
                        ValidationErrorKind::Required,
                        "fatal envelope must have a non-empty error message",
                    );
                }
            }
        }

        errs.into_result()
    }
}

/// Validates that a raw JSON value can be decoded as a valid [`Envelope`]
/// and that the `t` tag is present.
#[derive(Debug, Default)]
pub struct RawEnvelopeValidator;

impl Validator<serde_json::Value> for RawEnvelopeValidator {
    fn validate(&self, value: &serde_json::Value) -> Result<(), ValidationErrors> {
        let mut errs = ValidationErrors::new();

        let obj = match value.as_object() {
            Some(o) => o,
            None => {
                errs.add(
                    "",
                    ValidationErrorKind::InvalidFormat,
                    "envelope must be a JSON object",
                );
                return errs.into_result();
            }
        };

        // tag must be present
        match obj.get("t") {
            None => {
                errs.add(
                    "t",
                    ValidationErrorKind::Required,
                    "envelope must contain a 't' tag field",
                );
            }
            Some(t) => {
                if let Some(s) = t.as_str() {
                    let valid_tags = ["hello", "run", "event", "final", "fatal"];
                    if !valid_tags.contains(&s) {
                        errs.add(
                            "t",
                            ValidationErrorKind::InvalidFormat,
                            format!("unknown envelope tag '{s}'"),
                        );
                    }
                } else {
                    errs.add(
                        "t",
                        ValidationErrorKind::InvalidFormat,
                        "envelope tag 't' must be a string",
                    );
                }
            }
        }

        // hello must have contract_version
        if obj.get("t").and_then(|t| t.as_str()) == Some("hello")
            && !obj.contains_key("contract_version")
        {
            errs.add(
                "contract_version",
                ValidationErrorKind::Required,
                "hello envelope must contain contract_version",
            );
        }

        // run/event/final should have ref_id or id
        match obj.get("t").and_then(|t| t.as_str()) {
            Some("run") => {
                if !obj.contains_key("id") {
                    errs.add(
                        "id",
                        ValidationErrorKind::Required,
                        "run envelope must contain 'id'",
                    );
                }
            }
            Some("event" | "final") => {
                if !obj.contains_key("ref_id") {
                    errs.add(
                        "ref_id",
                        ValidationErrorKind::Required,
                        "event/final envelope must contain 'ref_id'",
                    );
                }
            }
            _ => {}
        }

        errs.into_result()
    }
}

/// Validates that the hello envelope version is compatible with our contract version.
pub fn validate_hello_version(envelope: &Envelope) -> Result<(), ValidationErrors> {
    let mut errs = ValidationErrors::new();

    if let Envelope::Hello {
        contract_version, ..
    } = envelope
    {
        if contract_version != CONTRACT_VERSION {
            // Allow same-major compat check
            let ours = CONTRACT_VERSION
                .strip_prefix("abp/v")
                .and_then(|r| r.split_once('.'))
                .map(|(m, _)| m);
            let theirs = contract_version
                .strip_prefix("abp/v")
                .and_then(|r| r.split_once('.'))
                .map(|(m, _)| m);
            if ours != theirs {
                errs.add(
                    "contract_version",
                    ValidationErrorKind::InvalidReference,
                    format!(
                        "incompatible contract version '{contract_version}', expected '{CONTRACT_VERSION}'"
                    ),
                );
            }
        }
    }

    errs.into_result()
}
