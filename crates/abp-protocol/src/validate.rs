// SPDX-License-Identifier: MIT OR Apache-2.0
//! Envelope validation for the ABP protocol.
//!
//! Provides [`EnvelopeValidator`] for checking individual envelopes and
//! validating sequences of envelopes against the expected protocol flow.

use std::fmt;

use crate::Envelope;

/// Recommended maximum serialized size (bytes) for a single envelope payload.
const MAX_RECOMMENDED_PAYLOAD: usize = 10 * 1024 * 1024; // 10 MiB

// ---------------------------------------------------------------------------
// Errors & warnings
// ---------------------------------------------------------------------------

/// A validation error indicating a hard violation of protocol rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    /// A required field is missing or null.
    MissingField {
        /// Name of the missing field.
        field: String,
    },
    /// A field has a value that does not match the expected format or range.
    InvalidValue {
        /// Name of the field.
        field: String,
        /// Actual value found.
        value: String,
        /// Description of what was expected.
        expected: String,
    },
    /// The `contract_version` could not be parsed as a valid protocol version.
    InvalidVersion {
        /// The version string that failed to parse.
        version: String,
    },
    /// A required field is present but empty.
    EmptyField {
        /// Name of the empty field.
        field: String,
    },
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingField { field } => write!(f, "missing required field: {field}"),
            Self::InvalidValue {
                field,
                value,
                expected,
            } => write!(f, "invalid value for {field}: got \"{value}\", expected {expected}"),
            Self::InvalidVersion { version } => {
                write!(f, "invalid protocol version: \"{version}\"")
            }
            Self::EmptyField { field } => write!(f, "field must not be empty: {field}"),
        }
    }
}

impl std::error::Error for ValidationError {}

/// A non-fatal warning about an envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationWarning {
    /// A field that has been deprecated was found.
    DeprecatedField {
        /// Name of the deprecated field.
        field: String,
    },
    /// The serialized envelope exceeds the recommended size.
    LargePayload {
        /// Actual size in bytes.
        size: usize,
        /// Recommended maximum size in bytes.
        max_recommended: usize,
    },
    /// An optional field that is commonly expected was absent.
    MissingOptionalField {
        /// Name of the optional field.
        field: String,
    },
}

impl fmt::Display for ValidationWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeprecatedField { field } => write!(f, "deprecated field: {field}"),
            Self::LargePayload {
                size,
                max_recommended,
            } => write!(
                f,
                "payload size {size} bytes exceeds recommended maximum of {max_recommended} bytes"
            ),
            Self::MissingOptionalField { field } => {
                write!(f, "missing optional field: {field}")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ValidationResult
// ---------------------------------------------------------------------------

/// The result of validating a single [`Envelope`].
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// `true` when there are no errors (warnings are allowed).
    pub valid: bool,
    /// Hard errors found during validation.
    pub errors: Vec<ValidationError>,
    /// Soft warnings found during validation.
    pub warnings: Vec<ValidationWarning>,
}

impl ValidationResult {
    fn new() -> Self {
        Self {
            valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn push_error(&mut self, e: ValidationError) {
        self.valid = false;
        self.errors.push(e);
    }

    fn push_warning(&mut self, w: ValidationWarning) {
        self.warnings.push(w);
    }
}

// ---------------------------------------------------------------------------
// SequenceError
// ---------------------------------------------------------------------------

/// An error found when validating an ordered sequence of envelopes against
/// the expected protocol flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SequenceError {
    /// The sequence contains no `Hello` envelope.
    MissingHello,
    /// No `Final` or `Fatal` envelope at the end of the sequence.
    MissingTerminal,
    /// A `Hello` envelope was found but not at position 0.
    HelloNotFirst {
        /// Zero-based index where the `Hello` was found.
        position: usize,
    },
    /// More than one terminal (`Final` or `Fatal`) envelope was found.
    MultipleTerminals,
    /// A `ref_id` did not match the expected run id.
    RefIdMismatch {
        /// The expected ref_id (from the `Run` envelope).
        expected: String,
        /// The actual ref_id found.
        found: String,
    },
    /// An `Event` appeared before a `Run` or after a terminal envelope.
    OutOfOrderEvents,
}

impl fmt::Display for SequenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingHello => write!(f, "sequence is missing a Hello envelope"),
            Self::MissingTerminal => {
                write!(f, "sequence has no terminal (Final or Fatal) envelope")
            }
            Self::HelloNotFirst { position } => {
                write!(f, "Hello envelope at position {position}, expected at 0")
            }
            Self::MultipleTerminals => write!(f, "sequence contains multiple terminal envelopes"),
            Self::RefIdMismatch { expected, found } => {
                write!(f, "ref_id mismatch: expected \"{expected}\", found \"{found}\"")
            }
            Self::OutOfOrderEvents => {
                write!(f, "Event envelope found outside the Run→Terminal window")
            }
        }
    }
}

impl std::error::Error for SequenceError {}

// ---------------------------------------------------------------------------
// EnvelopeValidator
// ---------------------------------------------------------------------------

/// Validates individual envelopes and envelope sequences against ABP protocol
/// rules.
#[derive(Debug, Clone)]
pub struct EnvelopeValidator {
    /// Maximum recommended payload size in bytes.
    max_recommended_payload: usize,
}

impl Default for EnvelopeValidator {
    fn default() -> Self {
        Self::new()
    }
}

impl EnvelopeValidator {
    /// Create a new validator with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self {
            max_recommended_payload: MAX_RECOMMENDED_PAYLOAD,
        }
    }

    /// Validate a single envelope, returning errors and warnings.
    #[must_use]
    pub fn validate(&self, envelope: &Envelope) -> ValidationResult {
        let mut result = ValidationResult::new();

        // Check serialized size for warnings.
        if let Ok(json) = serde_json::to_string(envelope)
            && json.len() > self.max_recommended_payload
        {
            result.push_warning(ValidationWarning::LargePayload {
                size: json.len(),
                max_recommended: self.max_recommended_payload,
            });
        }

        match envelope {
            Envelope::Hello {
                contract_version,
                backend,
                ..
            } => {
                // contract_version must be non-empty and parseable.
                if contract_version.is_empty() {
                    result.push_error(ValidationError::EmptyField {
                        field: "contract_version".into(),
                    });
                } else if crate::parse_version(contract_version).is_none() {
                    result.push_error(ValidationError::InvalidVersion {
                        version: contract_version.clone(),
                    });
                }

                // backend.id must be non-empty.
                if backend.id.is_empty() {
                    result.push_error(ValidationError::EmptyField {
                        field: "backend.id".into(),
                    });
                }

                // Warn if optional backend fields are missing.
                if backend.backend_version.is_none() {
                    result.push_warning(ValidationWarning::MissingOptionalField {
                        field: "backend.backend_version".into(),
                    });
                }
                if backend.adapter_version.is_none() {
                    result.push_warning(ValidationWarning::MissingOptionalField {
                        field: "backend.adapter_version".into(),
                    });
                }
            }

            Envelope::Run { id, work_order } => {
                if id.is_empty() {
                    result.push_error(ValidationError::EmptyField {
                        field: "id".into(),
                    });
                }
                if work_order.task.is_empty() {
                    result.push_error(ValidationError::EmptyField {
                        field: "work_order.task".into(),
                    });
                }
            }

            Envelope::Event { ref_id, .. } => {
                if ref_id.is_empty() {
                    result.push_error(ValidationError::EmptyField {
                        field: "ref_id".into(),
                    });
                }
            }

            Envelope::Final { ref_id, .. } => {
                if ref_id.is_empty() {
                    result.push_error(ValidationError::EmptyField {
                        field: "ref_id".into(),
                    });
                }
            }

            Envelope::Fatal { error, .. } => {
                if error.is_empty() {
                    result.push_error(ValidationError::EmptyField {
                        field: "error".into(),
                    });
                }
                // ref_id is optional on Fatal, but warn if missing.
                if let Envelope::Fatal { ref_id: None, .. } = envelope {
                    result.push_warning(ValidationWarning::MissingOptionalField {
                        field: "ref_id".into(),
                    });
                }
            }
        }

        result
    }

    /// Validate an ordered sequence of envelopes against the expected
    /// protocol flow: `Hello → Run → Event* → (Final | Fatal)`.
    #[must_use]
    pub fn validate_sequence(&self, envelopes: &[Envelope]) -> Vec<SequenceError> {
        let mut errors = Vec::new();

        if envelopes.is_empty() {
            errors.push(SequenceError::MissingHello);
            errors.push(SequenceError::MissingTerminal);
            return errors;
        }

        // -- Hello checks --
        let has_hello = envelopes
            .iter()
            .any(|e| matches!(e, Envelope::Hello { .. }));
        if !has_hello {
            errors.push(SequenceError::MissingHello);
        } else if !matches!(envelopes[0], Envelope::Hello { .. }) {
            // Hello exists but is not first.
            if let Some(pos) = envelopes
                .iter()
                .position(|e| matches!(e, Envelope::Hello { .. }))
            {
                errors.push(SequenceError::HelloNotFirst { position: pos });
            }
        }

        // -- Terminal checks --
        let terminal_positions: Vec<usize> = envelopes
            .iter()
            .enumerate()
            .filter_map(|(i, e)| match e {
                Envelope::Final { .. } | Envelope::Fatal { .. } => Some(i),
                _ => None,
            })
            .collect();

        if terminal_positions.is_empty() {
            errors.push(SequenceError::MissingTerminal);
        } else if terminal_positions.len() > 1 {
            errors.push(SequenceError::MultipleTerminals);
        }

        // -- Determine the run id for ref_id checks --
        let run_id: Option<&str> = envelopes.iter().find_map(|e| match e {
            Envelope::Run { id, .. } => Some(id.as_str()),
            _ => None,
        });

        // Find Run position for ordering checks.
        let run_pos = envelopes
            .iter()
            .position(|e| matches!(e, Envelope::Run { .. }));
        let terminal_pos = terminal_positions.first().copied();

        // -- Per-envelope ref_id and ordering checks --
        for (i, env) in envelopes.iter().enumerate() {
            match env {
                Envelope::Event { ref_id, .. } => {
                    // ref_id must match the Run id.
                    if let Some(expected) = run_id
                        && ref_id != expected
                    {
                        errors.push(SequenceError::RefIdMismatch {
                            expected: expected.to_string(),
                            found: ref_id.clone(),
                        });
                    }
                    // Event must appear after Run and before terminal.
                    let after_run = run_pos.is_some_and(|rp| i > rp);
                    let before_terminal = terminal_pos.is_none_or(|tp| i < tp);
                    if !after_run || !before_terminal {
                        errors.push(SequenceError::OutOfOrderEvents);
                    }
                }
                Envelope::Final { ref_id, .. } => {
                    if let Some(expected) = run_id
                        && ref_id != expected
                    {
                        errors.push(SequenceError::RefIdMismatch {
                            expected: expected.to_string(),
                            found: ref_id.clone(),
                        });
                    }
                }
                Envelope::Fatal { ref_id: Some(rid), .. } => {
                    if let Some(expected) = run_id
                        && rid != expected
                    {
                        errors.push(SequenceError::RefIdMismatch {
                            expected: expected.to_string(),
                            found: rid.clone(),
                        });
                    }
                }
                _ => {}
            }
        }

        // Deduplicate identical errors to keep output clean.
        errors.dedup();
        errors
    }
}
