// SPDX-License-Identifier: MIT OR Apache-2.0
//! Batch operation support for processing multiple envelopes at once.

use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::{Envelope, JsonlCodec};

/// Maximum number of envelopes allowed in a single batch.
pub const MAX_BATCH_SIZE: usize = 1000;

/// A batch of envelopes to process together.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchRequest {
    /// Unique identifier for this batch request.
    pub id: String,
    /// The envelopes to process.
    pub envelopes: Vec<Envelope>,
    /// ISO-8601 timestamp when the batch was created.
    pub created_at: String,
}

/// The result of processing an entire batch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchResponse {
    /// Identifier of the originating [`BatchRequest`].
    pub request_id: String,
    /// Per-envelope results, one for each input envelope.
    pub results: Vec<BatchResult>,
    /// Wall-clock duration of the entire batch in milliseconds.
    pub total_duration_ms: u64,
}

/// Outcome for a single envelope within a batch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchResult {
    /// Zero-based index of the envelope in the original request.
    pub index: usize,
    /// Whether this item succeeded, failed, or was skipped.
    pub status: BatchItemStatus,
    /// The response envelope, if applicable.
    pub envelope: Option<Envelope>,
}

/// Status of a single item in a batch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BatchItemStatus {
    /// The envelope was processed successfully.
    Success,
    /// The envelope could not be processed.
    Failed {
        /// Human-readable error description.
        error: String,
    },
    /// The envelope was intentionally skipped.
    Skipped {
        /// Reason the envelope was skipped.
        reason: String,
    },
}

/// Validation error for a batch request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatchValidationError {
    /// The batch contains no envelopes.
    EmptyBatch,
    /// The batch exceeds the maximum allowed size.
    TooManyItems {
        /// Actual number of items submitted.
        count: usize,
        /// Maximum allowed.
        max: usize,
    },
    /// An individual envelope is invalid.
    InvalidEnvelope {
        /// Zero-based index of the bad envelope.
        index: usize,
        /// Description of what went wrong.
        error: String,
    },
}

impl std::fmt::Display for BatchValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyBatch => write!(f, "batch is empty"),
            Self::TooManyItems { count, max } => {
                write!(f, "batch has {count} items, max is {max}")
            }
            Self::InvalidEnvelope { index, error } => {
                write!(f, "envelope at index {index} is invalid: {error}")
            }
        }
    }
}

impl std::error::Error for BatchValidationError {}

/// Processes a [`BatchRequest`] by encoding each envelope independently.
#[derive(Debug, Clone, Copy)]
pub struct BatchProcessor;

impl BatchProcessor {
    /// Create a new batch processor.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Process every envelope in the batch, collecting per-item results.
    ///
    /// Each envelope is independently serialized via [`JsonlCodec`] to verify
    /// it is well-formed. The response envelope is set to the original
    /// envelope on success.
    #[must_use]
    pub fn process(&self, request: BatchRequest) -> BatchResponse {
        let start = Instant::now();

        let results: Vec<BatchResult> = request
            .envelopes
            .into_iter()
            .enumerate()
            .map(|(index, envelope)| match JsonlCodec::encode(&envelope) {
                Ok(_) => BatchResult {
                    index,
                    status: BatchItemStatus::Success,
                    envelope: Some(envelope),
                },
                Err(e) => BatchResult {
                    index,
                    status: BatchItemStatus::Failed {
                        error: e.to_string(),
                    },
                    envelope: None,
                },
            })
            .collect();

        let elapsed = start.elapsed();

        BatchResponse {
            request_id: request.id,
            results,
            total_duration_ms: elapsed.as_millis() as u64,
        }
    }

    /// Validate a batch request without processing it.
    ///
    /// Checks that the batch is non-empty, within the size limit, and that
    /// every envelope can be serialized.
    #[must_use]
    pub fn validate_batch(&self, request: &BatchRequest) -> Vec<BatchValidationError> {
        let mut errors = Vec::new();

        if request.envelopes.is_empty() {
            errors.push(BatchValidationError::EmptyBatch);
        }

        if request.envelopes.len() > MAX_BATCH_SIZE {
            errors.push(BatchValidationError::TooManyItems {
                count: request.envelopes.len(),
                max: MAX_BATCH_SIZE,
            });
        }

        for (index, envelope) in request.envelopes.iter().enumerate() {
            if let Err(e) = JsonlCodec::encode(envelope) {
                errors.push(BatchValidationError::InvalidEnvelope {
                    index,
                    error: e.to_string(),
                });
            }
        }

        errors
    }
}

impl Default for BatchProcessor {
    fn default() -> Self {
        Self::new()
    }
}
