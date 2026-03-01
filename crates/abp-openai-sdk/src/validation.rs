// SPDX-License-Identifier: MIT OR Apache-2.0
//! Mapped-mode validation for early failure on unmappable parameters.
//!
//! When translating an OpenAI request to a non-OpenAI backend, certain
//! parameters are vendor-specific and cannot be faithfully mapped.
//! This module provides typed errors for those cases so callers fail
//! early with clear diagnostics rather than silently dropping parameters.

use serde::{Deserialize, Serialize};
use std::fmt;

// ---------------------------------------------------------------------------
// Validation error
// ---------------------------------------------------------------------------

/// An unmappable parameter detected during mapped-mode translation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UnmappableParam {
    /// Name of the parameter that cannot be mapped.
    pub param: String,
    /// Human-readable reason why this parameter is unmappable.
    pub reason: String,
}

impl fmt::Display for UnmappableParam {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unmappable parameter `{}`: {}", self.param, self.reason)
    }
}

impl std::error::Error for UnmappableParam {}

/// A collection of validation errors for a single request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ValidationErrors {
    /// The individual unmappable parameters found.
    pub errors: Vec<UnmappableParam>,
}

impl fmt::Display for ValidationErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "request contains {} unmappable parameter(s)", self.errors.len())?;
        for e in &self.errors {
            write!(f, "\n  - {e}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ValidationErrors {}

// ---------------------------------------------------------------------------
// Request with optional unmappable fields
// ---------------------------------------------------------------------------

/// Extended request fields that may be present in an OpenAI request
/// but are not universally supported across backends.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExtendedRequestFields {
    /// Whether `logprobs` was requested.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logprobs: Option<bool>,

    /// Top-logprobs count.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_logprobs: Option<u32>,

    /// Token-level logit biases.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logit_bias: Option<std::collections::BTreeMap<String, f64>>,

    /// Deterministic sampling seed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Validate extended request fields for mapped-mode compatibility.
///
/// Returns `Ok(())` if all fields are mappable, or `Err(ValidationErrors)`
/// listing every unmappable parameter found.
pub fn validate_for_mapped_mode(fields: &ExtendedRequestFields) -> Result<(), ValidationErrors> {
    let mut errors = Vec::new();

    if fields.logprobs == Some(true) || fields.top_logprobs.is_some() {
        errors.push(UnmappableParam {
            param: "logprobs".into(),
            reason: "log probabilities are not supported by most non-OpenAI backends".into(),
        });
    }

    if let Some(bias) = &fields.logit_bias
        && !bias.is_empty()
    {
        errors.push(UnmappableParam {
            param: "logit_bias".into(),
            reason: "token-level logit biases are backend-specific and cannot be mapped"
                .into(),
        });
    }

    if fields.seed.is_some() {
        errors.push(UnmappableParam {
            param: "seed".into(),
            reason: "deterministic seed is not supported by most non-OpenAI backends".into(),
        });
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(ValidationErrors { errors })
    }
}
