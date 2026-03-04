// SPDX-License-Identifier: MIT OR Apache-2.0
//! Validation error types.

use std::fmt;

/// A single validation error with field path and details.
#[derive(Debug, Clone)]
pub struct ValidationError {
    /// Dot-separated path to the offending field (e.g. `"config.max_budget_usd"`).
    pub path: String,
    /// Classification of the error.
    pub kind: ValidationErrorKind,
    /// Human-readable description.
    pub message: String,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

impl std::error::Error for ValidationError {}

/// Classification of a [`ValidationError`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationErrorKind {
    /// A required field is missing or empty.
    Required,
    /// A field value has an invalid format.
    InvalidFormat,
    /// A numeric value is outside its valid range.
    OutOfRange,
    /// A cross-reference between fields is invalid.
    InvalidReference,
    /// An application-specific validation rule failed.
    Custom,
}

impl fmt::Display for ValidationErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Required => write!(f, "required"),
            Self::InvalidFormat => write!(f, "invalid_format"),
            Self::OutOfRange => write!(f, "out_of_range"),
            Self::InvalidReference => write!(f, "invalid_reference"),
            Self::Custom => write!(f, "custom"),
        }
    }
}

/// A collection of [`ValidationError`]s.
#[derive(Debug, Clone, thiserror::Error)]
#[error("validation failed with {} error(s): {}", self.errors.len(), self.summary())]
pub struct ValidationErrors {
    errors: Vec<ValidationError>,
}

impl ValidationErrors {
    /// Create a new empty error collection.
    #[must_use]
    pub fn new() -> Self {
        Self { errors: vec![] }
    }

    /// Push a single error.
    pub fn push(&mut self, error: ValidationError) {
        self.errors.push(error);
    }

    /// Add an error with the given path, kind, and message.
    pub fn add(
        &mut self,
        path: impl Into<String>,
        kind: ValidationErrorKind,
        message: impl Into<String>,
    ) {
        self.errors.push(ValidationError {
            path: path.into(),
            kind,
            message: message.into(),
        });
    }

    /// Return `true` if no errors have been recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    /// Number of errors recorded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.errors.len()
    }

    /// Iterate over the contained errors.
    pub fn iter(&self) -> impl Iterator<Item = &ValidationError> {
        self.errors.iter()
    }

    /// Consume into the inner `Vec`.
    #[must_use]
    pub fn into_inner(self) -> Vec<ValidationError> {
        self.errors
    }

    /// Convert to `Result`: `Ok(())` when empty, `Err(self)` otherwise.
    ///
    /// # Errors
    ///
    /// Returns `Err(self)` if any validation errors were recorded.
    pub fn into_result(self) -> Result<(), Self> {
        if self.is_empty() { Ok(()) } else { Err(self) }
    }

    fn summary(&self) -> String {
        self.errors
            .iter()
            .map(|e| format!("[{}] {}", e.path, e.message))
            .collect::<Vec<_>>()
            .join("; ")
    }
}

impl Default for ValidationErrors {
    fn default() -> Self {
        Self::new()
    }
}
